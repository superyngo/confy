# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).


## [Unreleased]

### Unreleased Update - 2026-09-03 (3)

**Fixed**

- `web/build.mjs` shipped a **stale wasm core** without a word: it only *copies*
  `crates/confy-ffi/pkg/` into `web/pkg`/`web/dist` and never runs `wasm-pack`, so a
  `confy-core` fix (the notation-aware schema nudge clamp above) stayed invisible in every
  web/touch/Tauri/VS Code host until someone re-ran `wasm-pack build --target web` by
  hand — the fix was verified on the TUI binary and reported as landed while the browser
  still ran the old core. The build now compares `pkg/confy_ffi_bg.wasm`'s mtime against
  the newest `.rs` under `crates/confy-core/src` and `crates/confy-ffi/src` and prints a
  loud `WARNING: … ships a stale core` with the exact command to run. It warns rather than
  fails, so a TS-only rebuild still works without a Rust toolchain; `web/cf-build.sh`
  (CI/deploy) already rebuilt the wasm first and is unaffected.

**Added**

- Regression test `nudge_keeps_schema_grid_after_kind_switch_to_hex` (`schema_headless.rs`)
  covering the user-facing repro directly: `K`-switch a schema-constrained integer to hex,
  then nudge — the preview (`nudge_repr`, the web wheel/swipe path) and the keyboard
  `Intent::Nudge` must both walk the `multipleOf` grid and render in hex.

**Docs**

- New `WEBUI.md` § *Local build (`web/build.mjs` copies the wasm, it never rebuilds it)*
  spells out the boundary, the silent symptom (correct in the terminal, unchanged in the
  browser, no error anywhere), and the rule it implies: a `confy-core` change must have
  its verification re-run against a freshly built wasm, because that wasm *is* the web
  hosts' real binary (`functional_smoke.mjs` is the cheap way). `VSCODE.md` §
  *Build/test workflow* now marks its `wasm-pack` step as non-optional for a core change
  — skipping it stages a stale wasm into the extension's `media/`. `CLAUDE.md`'s build
  commands annotate `npm run build` as a wasm **copy**, never a rebuild.

### Unreleased Update - 2026-09-03 (2)

**Fixed**

- Value nudge (`←`/`→`, wheel, touch swipe) ignored every schema rule on a **non-decimal
  integer** and quietly rewrote its notation. `Session::schema_clamp_nudge` decoded the
  repr with `f64::from_str`, which rejects `0x…`/`0o…`/`0b…` outright — so a hex/octal/
  binary node fell through the early-return and stepped a bare ±1, ignoring `multipleOf`,
  `minimum` and `maximum` (`mask = 0xFF` with `multipleOf: 5` went to `0x100`, not
  `0x104`), and any value the clamp *did* produce was rendered as decimal. The clamp now
  decodes and re-renders in the **node's own notation** (new
  `schema_hint.rs::parse_repr`/`format_nudged_like`): the radix prefix and the authored
  hex digit case survive, and underscore grouping is re-applied (`1_000` +
  `multipleOf: 5` → `1_005`, not `1005`) — matching the grouping the unconstrained nudge
  already preserved. Floats keep the same guarantee (a grouped float no longer loses its
  `_`), and non-decimal integers now walk the schema grid and clamp inward to its bounds
  exactly like a decimal one.
- Detail panel (web/touch): a wheel/swipe nudge showed the stepped value in the panel but
  never reached the document or the tree. `web/panel.ts` committed on the input's `change`
  event only, and every engine resets its "text as of last change event" baseline on a
  *script* write — so the programmatically written nudge could not fire `change`, and
  blur/Enter committed nothing. The panel now also commits on blur whenever the field's
  text differs from what was rendered (one-shot guarded, so a typed edit still commits
  exactly once; Escape still cancels, since it restores the rendered text before
  blurring). Mirrors the tree inline editor, which always committed on blur and never had
  the bug.

### Unreleased Update - 2026-09-03

**Fixed**

- Detail popup (`i`): a branch's `Format:` line reported the *kind* word instead of the
  node's notation, so a dotted table (`[T/D]`) read `Format: table`, a standard scope
  (`[T/S]`) read `table`, and a multiline array (`[A/M]`) read `array`. The line is now
  derived from the node's `format` (`dotted` / `scope` / `multiline` / `inline` /
  `block`), with the kind word kept only as the `Plain`-format fallback (Root,
  array-of-tables entries). This matters beyond cosmetics: the popup is the TUI's
  recovery path for notation that a tree row does not spell out.
- Web kind badge: a TOML inline table and a YAML flow map badged a bare `inline` — the
  *kind* appeared nowhere. `NodeKind::InlineTable`'s label was the notation word
  `"inline"`, and a "don't repeat the label" guard then deleted the identical notation
  note. Both now read `{}·inline` / `{}·flow` on the row and `table · inline` /
  `table · flow` in the detail panel's Kind field. JSON was never affected (its objects
  are `NodeKind::Table` at both notations).

**Changed**

- Web kind badge: a container's label is now an **outline glyph** — `{}` for every
  table/map notation, `[]` for every array/sequence notation — with the notation kept in
  the note (`{}·scope`, `{}·dotted`, `[]·multi`, YAML `·block`/`·flow`). Scalars keep
  their short words (`str·"…"`, `int·0x`). An array-of-tables reads `[]·AoT`: it carries
  no `Format` of its own, so the note is the only thing separating `[[a]]` from a plain
  array under the shared glyph. YAML's inline note is now spelled **flow**, matching the
  term its `K` popup and legend already use. The kind as a word survives on the two
  surfaces with room for one: the badge's hover tooltip (composed with the schema hint)
  and the detail panel's Kind field. TUI rows are untouched — they keep the dense
  `[T/S]`-style kind tag, which no host shares.
- Web kind pill font size 10.5px → 10px, so the new `{}`/`[]` glyph labels sit in the pill
  without reading as visual noise.
- `tauri-plugin-dialog` 2.7.2 → 2.7.3, `tauri-plugin-fs` 2.5.1 → 2.5.2 (lockfile only).

**Security**

- Dependabot #11 (`glib` unsoundness in `VariantStrIter`, RUSTSEC/GHSA moderate, fixed in
  0.20) is **not reachable in any shipped build** and is dismissed rather than patched.
  `glib 0.18.5` enters the lockfile only through `gtk 0.18` ← `tao`/`wry`/`muda` ← `tauri`,
  all of which are `cfg(target_os = "linux")`-gated; `cargo tree -i glib` finds nothing on
  `x86_64-apple-darwin`, `aarch64-apple-darwin`, `x86_64-pc-windows-msvc`, or
  `aarch64-linux-android` — the four targets confy builds. It cannot be bumped either:
  `gtk 0.18` requires `glib ^0.18`, and the whole tauri v2 GTK stack moves together. This
  becomes real the day Linux is targeted (see CLAUDE.md § Known Risks).

## [v1.0.1] - 2026-09-02

**Deployment fix release.** No functional change to the editor itself; the release exists to
publish a corrected web deployment (the hosted site's build output was missing three boot
scripts).

### Fixed

- fix(web): `web/cf-build.sh` no longer wipes and re-copies `web/dist` with its own stale file
  list. `node build.mjs` already runs `web/assemble-dist.mjs` (the single source of truth for the
  runtime file set); the duplicated `cp` list in the Cloudflare build command predated the
  CSP-driven move of the boot scripts into external files, so the deployed site shipped an
  `index.html` referencing three files that were never copied. `entry-desktop.js` 404'd, so the
  coarse-pointer router never ran and a phone opening the site stayed on the desktop UI (the
  touch entry appeared to vanish); `register-sw.js` 404'd too, leaving the deployed PWA with no
  service worker registered.

## [v1.0.0] - 2026-09-02

**First stable release** — confy reaches 1.0.0 across the desktop app (Tauri), the terminal
(TUI/CLI), the web/touch UI, and the VS Code extension.

### Added
- feat(add): type picker replaces copy-cursor-kind add-node
- feat(web): PageUp/PageDown page the tree cursor (desktop + touch)
- feat(web): built-in samples rewritten around a shared backbone tree + per-format showcase
  branches (TOML dotted keys/AoT/radix/exponent/datetime, JSON comments/null/multiline arrays,
  YAML flow/block/literal/folded/anchor)
- refactor(tui,web): unified `?` Help overlay into a shared, i18n-driven Section/Row keymap
  model; new `docs/reference/KEYMAP.md` single source of truth, machine-checked against both
  implementations so a binding can no longer drift from the docs or the other surface

### Changed
- feat(nudge)!: removed boolean nudging everywhere; a schema-constrained number now steps along
  the schema's `multipleOf` grid (instead of freezing or snapping to the nearest multiple), with
  bounds clamping inward to the grid and new type-safety guards against retyping a node
- security(tauri): enabled a Content Security Policy for the desktop shell; inline boot
  `<script>` blocks moved to external files
- fix(tui): `$EDITOR` is shell-split, matches the open document's format for the scratch-file
  extension, and repaints via resize instead of clear after an external edit
- deps(tui): bumped `ratatui` 0.28 → 0.30 and `crossterm` 0.28 → 0.29

### Fixed
- fix(schema): a YAML anchor/alias/merge-key/tag no longer silences every schema-violation
  marker in the document; schema validation now lowers through a lenient conversion pass that
  skips (rather than aborts on) out-of-subset YAML nodes
- fix(web): remote `$schema` URL hints only upgrade `http://` to `https://` on an `https://`
  page, fixing schema loading on local dev servers and Tauri's Windows origin
- fix(convert): converting an empty document to YAML no longer aborts
- fix(cli): `confy convert` refuses to silently overwrite an existing destination without
  confirmation or `--yes`
- fix(tui): a leading UTF-8 BOM no longer makes a file unloadable; saves are now atomic
  (temp file + rename) so a crash mid-write can't truncate a config
- fix(core): container nesting is capped at 256 levels, so hostile/deeply-nested input is a
  parse error instead of a stack overflow
- fix(web): the `E` (edit-external) shortcut now works on web/touch/VS Code, matching the TUI
- fix(pointer): drag/gesture drops resolve through `PasteSlot` end to end (ADR 0010); inline/flow
  containers regain their drop-into band
- fix(i18n): `core.add.placeholder` notice now says `F2`, matching the actual rename binding

### Docs
- docs(claude): refreshed module map against the tree
- docs: recorded the lenient schema lowering and schema-grid nudge across the reference docs

### Unreleased Update — 2026-09-02T15:40:00Z
- docs(nudge): record the schema-grid nudge step across the reference docs and make the Help
  overlay's own row honest about it. `KEYMAP.md`'s `ArrowRight`/`ArrowLeft` rows now note that a
  schema `multipleOf` *is* the step and that bounds clamp inward to that grid; `TUI.md` gains the
  three type guards (fractional `multipleOf` ignored on an integer-style value, a float keeping
  its decimal point, so a nudge never retypes the node); `WEBUI.md` names `nudge_repr` as the same
  core pipeline the TUI's `←/→` walks. The `help.row.nudge` catalog string changed in both
  languages ("±1 number" → "Step a number (schema step if any)" / "數字加減一階（有 schema
  時依其級距）"), since `±1` is no longer the step on a constrained node — verified rendering
  aligned and inside the popup on the real binary in `en` and `zh-TW`.

### Unreleased Update — 2026-09-02T15:10:00Z
- fix(schema): a schema-constrained number no longer **freezes** under nudge (`←`/`→`, mouse
  wheel, touch swipe). `Session::schema_clamp_nudge` snapped the nudged value to the *nearest*
  `multipleOf`, while the step it snapped was only ±1 — on any grid coarser than 2 the snap
  always landed back on the value the step came from, so the value never moved. The built-in
  sample's `schema.poll_ms` (`multipleOf: 5`) was stuck at `255` in **both** directions;
  `multipleOf: 2` was stuck going down, and a float's `10^-places` step froze against any
  fractional grid the same way. The nudge now **steps along the schema's grid**: an on-grid
  value moves `delta` whole steps (253 → 255 → 260 → 265), an off-grid value aligns in the
  nudge's own direction on the first step (253 up → 255, down → 250), and a multi-step delta
  (web wheel bursts) moves that many steps. `minimum`/`maximum` now clamp **inward to the
  nearest in-range grid point**, so parking at a bound can't leave a value the schema itself
  rejects (nor oscillate against the snap). Three type-safety guards came with it: a
  fractional `multipleOf` is ignored on an integer-style repr (a nudge must not retype an
  Integer node as a Float), a whole-numbered float result keeps its decimal point (`5` → `5.0`,
  which previously retyped a Float node as Integer), and a grid's own decimal count sets the
  output precision so a `0.1` grid can't surface float noise (`0.30000000000000004`). Without a
  schema constraint the step is unchanged (±1, or ±1 at the displayed precision for a float).
  New `schema_hint.rs::format_nudged`; `schema_clamp_nudge` takes the pre-nudge repr + delta,
  and both callers (`nudge`, and the Web/touch `nudge_repr` query) pass them — no host, FFI or
  TypeScript signature changed.

### Unreleased Update — 2026-09-02T14:10:00Z
- docs: record the lenient schema lowering across the reference docs. `CONTEXT.md`'s **JSON
  projection** and **Violation** glossary entries now name `convert::tree_to_value_lenient` +
  `value_bridge::bridge` as the lowering pair and state that a YAML opaque node carries no
  Violation while its siblings/ancestors do; `BEHAVIOR_MATRIX.md` §8's YAML-opaque invariant
  gains the "schema validation skips them, conversion aborts" split; `CLAUDE.md`'s module map
  and JSON Schema section point at the lenient variant.

### Unreleased Update — 2026-09-02T13:30:00Z
- fix(schema): a YAML file containing an anchor, alias, `<<:` merge key or tag no longer loses
  **every** schema-violation cue. `Session::revalidate_schema` lowered through
  `ConfigDocument::to_value()`, which aborts the whole document on the first opaque
  (out-of-subset) node, so validation bailed and no row carried a `violations` entry — no ▲/△
  marker, no dashed warn frame, no KIND `!`, no "N schema warning(s)" status. The Detail
  popup/panel kept showing schema info and constraint text (those resolve the sub-schema by
  path and never lower the document), which is exactly how the bug hid; TOML and JSON were
  unaffected because neither has opaque nodes. Validation now lowers through a new
  `convert::tree_to_value_lenient`, which **skips** an opaque node instead of aborting;
  `value_bridge::walk` skips the same nodes so the Node↔Value pairing stays 1:1 and every
  sibling *after* an anchor still resolves to its own path (a skipped sequence element shifts
  the JSON array, and the pointer map translates it back correctly). The opaque node itself is
  never flagged — confy cannot decode its value. `convert()` is unchanged and still aborts:
  dropping data is fine for an advisory validation pass, not for writing a converted file.
  Reproduced and confirmed fixed on the real `confy` binary (`▲ port` / `[S:str!]` /
  "1 schema warning(s)" now render for a YAML doc with `&pin`/`*pin`); the built-in YAML
  **sample** hit this on every load, since it ends with `pinned: &pin "confy"`.

### Unreleased Update — 2026-09-02T11:05:00Z
- fix(web): the built-in sample no longer reports "Schema failed to load: NetworkError" on an
  http origin. `resolveSchemaFetchRequest`'s `http://` → `https://` mixed-content upgrade was
  unconditional, but the sample's `$schema` is derived from `location.href` (`samples.ts`), so a
  local dev server (`http://localhost:8080`) or Tauri's Windows origin (`http://tauri.localhost`)
  got rewritten to an https URL nothing serves. The upgrade is now gated on the *page* being
  https — the only case where the browser blocks the plain-http fetch as mixed content — via a
  new exported `upgradeForMixedContent()`; `host-io.spec.mjs` covers both page protocols.

### Unreleased Update — 2026-09-02T09:45:00Z
- docs(claude): refresh the module map against the tree. Adds the 16 source files it had
  drifted past — `session/action_menu.rs`, `session/add_picker.rs`,
  `tui/overlay_action_menu.rs`, `tui/overlay_add_picker.rs`, and the shared web modules
  `host-io.ts`, `key-intent.ts`, `mode.ts`, `escape.ts`, `kind-labels.ts`, `samples.ts`,
  `help-content.ts`, `convert-dialog.ts`, `typefilter.ts`, `fab.ts`,
  `action-menu-items.ts`/`add-picker-items.ts` — plus the new entry scripts, lists all three
  `confy-tui/tests/` files (was only `convert_cli.rs`), and corrects the
  `functional_smoke.mjs` check count (92 → 128).

### Unreleased Update — 2026-09-02T09:20:00Z
- security(tauri): set a real `app.security.csp` (was `null`, i.e. no CSP at all) —
  `default-src 'self'`, `script-src 'self' 'wasm-unsafe-eval'`, `object-src 'none'`,
  `base-uri 'self'`, `frame-ancestors 'none'`, with `https:`/`http:`/`ipc:` allowed in
  `connect-src` for remote `$schema` hints, Open-from-URL and Tauri IPC. The desktop shell loads
  remote content, and `fs:scope` is intentionally `**`, so this is the layer that keeps an
  escaping bug from reaching the disk. Rationale + the full directive breakdown are in
  `docs/reference/TAURI.md §Content Security Policy`.
- refactor(web): the two inline boot `<script>` blocks in `index.html`/`touch.html` moved to
  external `entry-desktop.js` / `entry-touch.js` / `register-sw.js` (added to
  `assemble-dist.mjs`). Required by the CSP above: verified under headless Chrome served with
  the exact policy that both blocks were being *blocked* ("Executing inline script violates …"),
  which would have silently killed the desktop↔touch redirect and the PWA registration; after
  the move the same load reports no CSP violations and the tree renders.

### Unreleased Update — 2026-09-02T08:40:00Z
- fix(convert): converting an empty document to YAML no longer aborts with `internal: converted
  output did not re-parse: expected a mapping key, found Some(L_BRACE)`. `render_yaml` emitted
  `{}` for an empty root, which the YAML backend (no root-level flow collections in its subset)
  rejects in the reparse safety net; an empty map root now renders as an empty document, which
  the backend loads as an empty mapping. New unit test walks every (from, to) pair on an empty
  TOML/YAML/JSON source.

### Unreleased Update — 2026-09-02T08:25:00Z
- fix(cli): `confy convert` no longer overwrites an existing destination silently. It now asks
  `<out> already exists. Overwrite it? [y/N]` on a TTY and refuses on a pipe unless `--yes` is
  given — the same contract the lossy-warning prompt already had. New i18n keys
  `cli.convert.overwrite` / `cli.convert.refuse-overwrite` (en + zh-TW); the two prompts share
  one `confirm_or_bail` helper. Two new `convert_cli.rs` integration tests.

### Unreleased Update — 2026-09-02T08:05:00Z
- fix(tui): a leading UTF-8 BOM no longer makes a file unloadable (`parsing bom.json` error).
  `load_document` strips it, remembers it (`LoadedDocument::bom`), and `App::save`/`confy
  convert` put it back on write — verified with `confy convert` on a BOM'd `.json`.
- fix(tui): saves are atomic. `App::save`, the TUI `C` convert output, and `confy convert` now
  go through `confy_tui::write_document` — write to a sibling `.confy-*.tmp`, fsync, carry over
  the destination's Unix permission bits, rename over the target — instead of a bare
  `fs::write` that could leave a truncated config behind on a crash or kill.

### Unreleased Update — 2026-09-02T07:40:00Z
- fix(core): cap container nesting at `MAX_NESTING_DEPTH` (256) in all three backends. A
  `[[[[…` 100k deep used to abort the TUI with `fatal runtime error: stack overflow` (and trap
  the wasm instance in the web hosts) for `.json`, `.yaml` and `.toml` alike; it is now a plain
  parse error at load and a rejected (atomic, doc-untouched) `Replace`/`Insert` on the `$EDITOR`
  path. JSON/YAML count depth in their parsers; TOML pre-scans brackets (string/comment-aware)
  before taplo. New `tests/hostile_input.rs` pins the boundary (255 loads, 257 rejects, 200k
  does not overflow) — verified on the real `confy convert` binary.

### Unreleased Update — 2026-09-02T06:55:00Z
- fix(tui): `$EDITOR` is now shell-split (`shell-words`), so values carrying flags —
  `EDITOR="code --wait"`, `"emacsclient -t"` — launch instead of failing with
  "launching editor: code --wait"; an empty `$EDITOR` falls through to `$VISUAL`/`vi`. The
  scratch file now carries the open document's extension (`.json`/`.yaml`, not always `.toml`)
  so the editor applies the right syntax mode.
- fix(tui): after an external edit the screen is repainted via `Terminal::resize` instead of
  `Terminal::clear` — ratatui 0.30's `clear` first queries the cursor position, which aborted the
  whole session ("cursor position could not be read") on PTYs that don't answer DSR. Verified on
  the real binary under a supervised PTY: the `E` round trip now returns to the tree with the
  edited rows.
- test(tui): the three tests that mutate `$EDITOR` share a `parking_lot` mutex
  (`editor::tests::ENV_LOCK`) — they raced under the parallel test runner.

### Unreleased Update — 2026-09-02T06:25:00Z
- deps(tui): bump `ratatui` 0.28 → 0.30 and `crossterm` 0.28 → 0.29. Clears the two `unsound`
  advisories `cargo audit` raised through ratatui's old `lru 0.12` (RUSTSEC-2026-0253,
  RUSTSEC-2026-0002) and the unmaintained `paste`; the remaining 17 warnings are all GTK/unic
  crates behind Tauri on Linux, which confy doesn't target.
- build: track `Cargo.lock` (removed from `.gitignore`). The workspace ships binaries (`confy`,
  the Tauri app), so the lockfile is what makes release builds reproducible and what
  `rust-ci.yml`'s `cargo audit` step actually audits — previously it audited a fresh resolution
  on every run. `web/` and `editors/vscode/` already tracked their `package-lock.json`.

### Unreleased Update — 2026-09-02T06:10:00Z
- style(rust): `cargo fmt` pass over `tui/keys.rs` + `session/session.rs` — the Help overlay
  refactor (`811e5b6`) landed unformatted, so `rust-ci.yml`'s `cargo fmt --check` gate was red.

### Unreleased Update — 2026-09-02T04:00:02Z
- feat(web): rewrote the built-in sample end-to-end (`web/samples.ts`) around one **shared
  backbone tree** (about/basics/servers/types/schema/links — identical keys/values in
  TOML/JSON/YAML, so the format pill reads as the same doc in three coats) plus a per-format
  `showcase` branch exercising each backend's exclusive notations: TOML dotted keys/AoT/radix
  ints/exponent/inf/datetime, JSON `//` comments + null + multiline array, YAML flow seq/block
  map/literal/folded/anchor+alias. Comments — not narrated values — are the teaching voice;
  the word "banana" is seeded 5× across `basics`/`servers` for the `/` filter-highlight demo.
  The JSON sample now deliberately carries comments (JSONC), which fires the host's
  comment-advisory underline **and** still resolves the `$schema` hint (`schema/hints.rs`
  detects through the JSONC-aware parser) — the E-2 no-dropped-notice repro shape.
  `schema.advanced` ships collapsed-by-default with a schema-invalid seed so the collapsed
  `schema` parent demos `has_descendant_violation` (E-3).
- feat(web): `web/schema-sample.json` gains `$defs`/`$ref` (the `editor` enum now resolves
  through `#/$defs/editorName`, surfacing a schema `description`), a `multipleOf`-bounded
  `poll_ms` with a description, and a new `pattern`-constrained `schema.advanced.retry_pattern`
  (seeded invalid: `"abc"`) — 3 seeded violations across 3 constraint kinds, all demoed on the
  collapsed `schema.advanced` branch.
- feat(about): the About panel now linkifies **every** URL (`help-content.ts` regex widened to
  global) and `ABOUT_TEXT`/`ABOUT_TEXT_ZH_TW` gain four resource lines — Live demo, VS Code
  Marketplace, Open VSX, MS Store — so all hosts (TUI/web/touch/Tauri/VS Code) show clickable
  links; README's Desktop-app Windows bullet gains the Microsoft Store link.

### Unreleased Update — 2026-09-02T10:35:00Z
- refactor(tui,web): unified the `?` Help overlay's keymap content into a shared, i18n-driven Section/Row model (Navigation/Selection/Edit/File & App), grouping and two-columning what was an ad-hoc four-column TUI layout and a `·`-strung Web cheatsheet. New `help.section.*`/`help.row.*` catalog entries in `i18n/{en,zh-TW}.json` (119 keys) back both `crates/confy-tui/src/tui/keys.rs::help_sections` (rendered with `unicode-width`-aligned columns inside a now-padded popup, `Padding::new(2, 2, 1, 1)`, fixing the missing gap between border/title and content) and `web/help-content.ts` (rendered as a CSS grid, `.help-grid`/`.help-key`/`.help-desc`, replacing the old `<pre>` text blob and its 4 now-deleted `HELP_TEXT*` constants). Reflowed the 6 `web.help.legend.*` Kind-legend strings to one label/description pair per line. zh-TW wording tightened for the external-editor rows ("編輯器"/"強制開啟編輯器" replacing the looser "多行對話框"/"強制對話框"). `docs/reference/KEYMAP.md` gains "Help overlay parity" and "Editor (inline/external) parity" sections documenting the shared-row model, the VS Code variant split, and why the TUI/Web Kind-legend vocabularies stay intentionally un-unified. See `docs/superpowers/plans/2026-09-02-HELP_OVERLAY_PLAN.md`.

### Unreleased Update — 2026-09-02T01:34:17Z
- docs(keymap): new `docs/reference/KEYMAP.md` — the TUI ↔ Web **single source of truth** for keyboard bindings. Documents the full normal-mode key table (49 rows: canonical key, TUI `KeyAction`, Web `KeyResolution`, status), the deliberate divergences (`w` TUI-only, `g`/`G` and `+`/`-` web-only, `l` lang picker vs toolbar dropdown, `Tab` `.json`/`.jsonc` toggle vs the `Jsonc` `<select>` option, `Ctrl+O` web-only, `~` TUI-only, `q` suppressed under `vshost`), and the per-surface input-handling differences (web's native `<input>` inline edit with `stopPropagation()` vs the TUI's core-driven buffer — which is why `EditCursor*`/`EditDelete` are declared but unused in `web/types.ts`; the three presentations of the one `external_edit` handshake; clipboard-guard ownership; `treePageStep` vs `terminal_height / 2`). Registered in `docs/reference/README.md` and cross-referenced from `TUI.md`/`WEBUI.md`.
- test(keymap): the KEYMAP.md table is **machine-checked against both implementations**, so a binding can no longer drift from the docs or from the other surface — the root cause of the `E` regression fixed in the previous entry. `crates/confy-tui/src/tui/keys.rs` gains four `keymap_doc_*` tests (TUI column vs `map_key`, status-column consistency, completeness scan, unbound-really-unbound) and `web/keymap-parity.spec.mjs` runs the equivalent four against `resolveKeyIntent`. Both parse the same markdown table between `<!-- KEYMAP-TABLE:BEGIN/END -->` markers. The completeness scans reject any binding present in an implementation but absent from the doc; the TUI scan skips `Ctrl+<letter>` combinations that are mere modifier-wildcard aliases of the unmodified key (`map_key`'s char arms match `_` modifiers) so only genuinely distinct Ctrl bindings like `Ctrl+S` are required. `KeyAction` now derives `Debug, PartialEq, Eq` to supply the variant names. Verified by six mutation tests: deleting a row, corrupting either binding column, corrupting the status column, deleting the `E` case from the implementation, and adding an undocumented binding each fail the appropriate guard.

### Unreleased Update — 2026-09-02T01:19:45Z
- fix(web): the `E` (Shift+E) shortcut did nothing on the web/touch/VS Code hosts — `resolveKeyIntent` (`web/key-intent.ts`) had no `"E"` case, so `onKey` bailed on the `null` resolution and the keystroke was silently dropped. `E` now resolves to `Intent::BeginEditExternal`, aligning with the TUI's `E` -> `KeyAction::EditExternal` (`crates/confy-tui/src/tui/keys.rs`): it force-opens the popup/external editor on **any** node, regardless of kind or schema, whereas plain `e` only routes External when core's `edit_target_kind()` says so (multiline string / comment). This was the only forced-external path missing from the web keyboard — the panel's "Editor" button already sent the same intent. Desktop opens `#ext-modal`, touch opens its `.ext-sheet`, both via the existing `snap.external_edit` handshake; core's `begin_external_edit` owns the clipboard-armed guard, so no host-side check was added. Documented in the Help overlay (all four `web/help-content.ts` variants, en + zh-TW) and `docs/reference/WEBUI.md`; new `web/key-intent.spec.mjs` case.

### Unreleased Update — 2026-09-02T00:00:00Z
- fix(web): remote `$schema` URL hints with an `http://` scheme never loaded on the web/touch hosts — `resolveSchemaFetchRequest` fetched the URL directly from the browser, and an https page blocks an `http://` fetch as mixed content before json-schema.org's 301 redirect to https can run, surfacing "Schema failed to load: Failed to fetch" (TUI/VS Code resolve natively, so only the browser hosts were affected). `http://` hints are now upgraded to `https://` before the browser fetch — the https endpoints of schema hosts (e.g. `https://json-schema.org/draft-07/schema`) send `access-control-allow-origin: *`, so the fetch succeeds. Tauri/VS Code branches unchanged (native fetch, no mixed-content restriction). Covered by two new `web/host-io.spec.mjs` checks.

### Changed (2026-09-01)
- feat(nudge)!: remove boolean nudging everywhere (TUI arrows, web keyboard, wheel); bools edit only via the true/false picker — `nudge_scalar` no longer touches `Bool`
- feat(web): wheel/swipe value nudge now requires inline-edit focus, captures all wheel ticks/horizontal swipes page-wide while armed, and writes via the new stateless `nudge_repr` core query (single `CommitEdit` on blur/Enter — no per-tick document mutation)

## [v0.32.0] - 2026-09-01
### Added
- feat(add): type picker replaces copy-cursor-kind add-node
- feat(web): PageUp/PageDown page the tree cursor (desktop + touch)

### Fixed
- fix(pointer): resolve drag/gesture drops through PasteSlot end to end (ADR 0010)
- fix(add): seed datetimes with system clock; drop forced rename on container add
- fix(desktop): disable Tauri native drag-drop to restore HTML5 dnd
- fix(touch): scroll the tree to follow keyboard nav, including paste mode
- fix(i18n): core.add.placeholder notice says F2, not e

### Changed
- ci(msstore): revert to auto-committing the Store submission

### Docs
- docs(debug): freeze the pointer-drop PasteSlot alignment probe into the repo
- docs: RELEASES.md version updates for v0.31.1

### Unreleased Update — 2026-09-01T12:40:40Z
- fix(i18n): the `core.add.placeholder` notice ("added placeholder node — rename with …")
  told the user to press `e` in both `en.json` and `zh-TW.json`, but the actual rename
  binding is `F2` (see `README.md`'s keybinding table and the TUI's `KeyCode::F(2)`
  handler). Both locale strings now say `F2`.

### Unreleased Update — 2026-09-01T20:30:00Z
- **feat(web): PageUp/PageDown page the tree cursor (desktop + touch).** Core already supported
  paging via `Intent::PageUp(usize)`/`Intent::PageDown(usize)` (`crates/confy-core/src/session/session.rs`,
  wired into the TUI with `page_size = terminal_height / 2`), but the web UI's `key-intent.ts`
  had no `PageUp`/`PageDown` case in the tree's normal-mode key switch, so pressing them only
  scrolled the browser natively. Added a `tree-page` resolution kind plus a new exported
  `treePageStep()` (mirrors `typeFilterPageStep`'s DOM-derived scroll-ratio technique, halved to
  match the TUI's `height / 2` convention — no hardcoded row height), dispatched through the
  existing `navSelect`/`touchNavSelect` wrappers in both `web/ui.ts` and `web/touch/app.ts` so
  clipboard/selection/scroll-follow behavior stays identical to every other nav intent. Help
  text (`web/help-content.ts`, all four locale/host variants) now documents `PgUp/PgDn page`.

### Unreleased Update — 2026-09-01T12:00:00Z
- **fix(touch): keyboard cursor/jump-key navigation now scrolls the tree pane, including
  paste (cut/copy) mode.** `web/touch/app.ts`'s `render()` re-applies the tree pane's captured
  `scrollTop` verbatim after every `innerHTML` rebuild (so a tap's re-render never snaps the
  pane back to the top) — but that also meant an external/Bluetooth keyboard's arrows, j/k,
  g/G/Home/End or Shift+↑/↓ could move the focus past a viewport edge with **no scroll at all**;
  reproduced in a real Chromium (594px pane, cursor row landing at y≈1657, `scrollTop` stuck at
  0). Desktop already gets this for free from `renderTree`'s own `scrollIntoView`
  (`web/render.ts`); touch had no equivalent — and even desktop never scrolled to the paste-mode
  insertion slot, only the cursor. New `scrollFocusIntoView()` (touch) applies minimal
  ("sticky cursor") scrolling — the anchor moves freely inside the viewport and `scrollTop`
  changes only by the exact overflow when it crosses an edge, never centered — hand-rolled
  against `treePane.scrollTop` rather than `Element.scrollIntoView`, which also scrolls
  ancestors/the page (the `.app` shell is `position:absolute`, so that would slide it out from
  under its own bottom-anchored sheets). In paste mode, where arrows move the insertion slot and
  not the cursor (`Session::move_selection_to`'s `PasteSlot`), the anchor is the
  `.reorder-line` for an `After` slot and the target row for `Into`, matching what the paste-mode
  cue actually draws.
- **fix(web): `Home`/`g` (and `k` from the first row) no longer leave an invisible cursor.**
  `Session::cursor_home` can legitimately land the cursor on the document's root row (empty
  path) — the TUI draws it, so core is right for its own host — but neither web host draws
  the root row (`web/render.ts`, `web/touch/render.ts`), so the cursor bar simply vanished on
  both desktop and touch. New shared `drawnCursorFallback()` (`web/path-utils.ts`) re-targets
  the first drawn row whenever a keyboard nav dispatch leaves the cursor there; wired into both
  `web/ui.ts`'s `navSelect` and `web/touch/app.ts`'s `touchNavSelect`, before the `SetSelection`
  that collapses the selection onto the cursor, so the two never desync. Paste mode's analogous
  `Into(root)` target (the slot `Home` arms when armed) is now drawn as an insertion line at the
  very top of the tree, since it has no row of its own to outline.
- New `web/touch-key-scroll.spec.mjs` (extract-real-body + fake-DOM convention): minimal-scroll
  at both edges, no-op inside the viewport, paste-mode `After`/`Into`/`Into(root)` anchoring, and
  `drawnCursorFallback` correctness — plus source-shape checks that every resolved key runs the
  scroll follow and that it's never implemented via `Element.scrollIntoView`.

### Unreleased Update — 2026-09-01T10:24:00Z
- **fix(pointer): drag/gesture drops resolve through `PasteSlot` end to end, and
  inline/flow containers regain their drop-into band (ADR 0010).** The mouse grip
  drag (`web/dnd.ts`) and the touch grip reorder (`web/touch/app.ts`) only asked
  core whether a hover was an `Into`; the rest of the destination was hand-rolled
  as "before/after this row ⇒ a sibling in `parentOf(path)` at
  `siblingIndex(...) ± 1`" with a local `rel < 0.5` split, sent as
  `MoveSelectionTo { target, index }` — bypassing `slot_target`, the resolution
  the TUI and every keyboard/armed-paste surface has always used. That disagreed
  with core three ways, all measured against the real wasm core:
  - **Level.** `After(<expanded branch>)` inserts as that branch's **first
    child** (`resolve_target`), not a sibling one level up. Dragging a root key
    into the gap under an expanded `[b]` therefore aimed at root index 2 and, in
    TOML, failed outright — notice `paste error: a key here would be captured by
    the table above it`, document untouched — while an armed cut+paste released
    at the *same pixel* correctly made it `[b]`'s first child. This is the
    reported "expanded branch 和其 child 之間的間隙插到 branch 的 sibling 位置"
    bug.
  - **Level, upward.** Hovering the top band of the row *after* an expanded
    branch: core says "inside, after its last descendant"; the drag said "root,
    before this row". One visual gap, two levels, no design covering which wins.
  - **Threshold.** Core's leaf before/after boundary is `0.75`; the drag used
    `0.5`, so every `rel ∈ [0.5, 0.75)` band classified drag and paste opposite
    ways.

  `Intent::MoveSelectionTo` now carries `{ sources, slot, cut }` — the same
  `PasteSlot` an armed paste uses — and `Session::move_selection_to` resolves it
  with `slot_target`, ignoring a slot whose row is no longer visible and
  rejecting self/self-subtree drops on the resolved parent. Both hosts keep a
  single `PasteSlot` as their whole drop-target state; `parentOf`,
  `siblingIndex`, `child_count` and the `0.5` split are gone from both drop
  paths, and touch's outside-any-row fallback clamps `rel` and still asks
  `pointer_slot` instead of inventing a mode. A new headless test
  (`move_selection_to_and_paste_agree_for_every_pointer_band`) pins drag/paste
  equality across every row × band so no surface can re-derive a target again.

  Trade-off, accepted and recorded in ADR 0010: the pointer loses the one target
  its old math could express that the slot model cannot — "root level, past an
  expanded branch's whole subtree". That level dimension was deliberately
  excluded from the TUI slot model; reaching it means collapsing the branch
  first, exactly as in the TUI.

  Separately, `Session::pointer_slot` no longer withholds `Into` from a
  `Format::Inline` container (TOML inline table/array, YAML flow map/sequence).
  `paste_slots()` always offered it to the keyboard and core always accepted it
  (`t = { x = 1 }` + `Into(t)` → `t = { x = 1, k = 9 }`), so the pointer was the
  only surface unable to aim at a legal, keyboard-reachable target — and the
  guard was self-inconsistent, since a *collapsed* multi-line branch (equally one
  row, equally invisible children) kept its band. This is the reported
  "inline/flow container 無法被選取框定位" bug.
- **fix(web+touch): the insertion line is drawn at the level it actually inserts
  at.** The TUI has always indented its green paste line one step deeper for
  `After(<expanded branch>)` (`paste_line_row`'s `row.depth + 1`); the web
  `#dropLine`/`#pasteTargetLine` and the touch `.reorder-line` used the hovered
  row's own indent unconditionally (touch's line had no horizontal position at
  all), so even the *keyboard*-driven cue pointed a level too shallow at the one
  gap where the level is ambiguous. New shared `web/slot-line.ts`
  (`slotLineIndentPx`) is now the single rule behind all three cues. The web drag
  line is also drawn under **the slot's** row rather than the hovered one, since
  the two differ whenever a top band resolves to a flattened predecessor slot.

### Unreleased Update — 2026-09-01T08:38:45Z
- **fix(add): seed datetime scalars with the system clock, and stop forcing
  a rename after adding a container.** Two follow-ups to the Add-type picker
  above:
  - `OffsetDatetime`/`LocalDatetime`/`LocalDate`/`LocalTime` used to seed a
    fixed `1970-01-01T00:00:00Z` stub; they now seed the system clock's
    current UTC instant instead. `std::time::SystemTime::now()` traps at
    runtime on `wasm32-unknown-unknown` (confirmed directly: it compiles,
    but the call itself is an `unreachable` trap), which is the target the
    web/touch/VS Code/Tauri UIs all run `confy-core` as, so that target
    reads the JS `Date.now()` clock via a new wasm32-only `js-sys`
    dependency; the TUI (a native process) keeps using `SystemTime`. No
    date/timezone crate added — day/month/year is computed with Howard
    Hinnant's public-domain `civil_from_days` algorithm.
  - Adding a container (table/array/inline-table/array-of-tables) no longer
    forces the cursor into the rename editor — it lands inert with its
    auto-numbered `placeholder` key and the pre-existing "added placeholder
    node — rename with e" notice, matching how a bare array-element
    container was already handled. Previously Escape right after a
    container add would roll the insert back (via the rename surface's
    `created_on_add`); that shortcut is gone too — undo (`u`) removes it
    same as any other change.

### Unreleased Update — 2026-09-01T08:09:07Z
- **feat(add): replace "copy the cursor's kind" add-node with a type picker.**
  `a` / "Add child" / "Append sibling" (TUI, web desktop, web touch/VS Code)
  now open a keyboard/pointer-navigable **Add-type picker** (`Mode::AddPicker`)
  listing every simple type, container, and comment legal at the insertion
  point (filtered by the parent's kind/notation — e.g. an `[[array-of-tables]]`
  group only offers "Table entry"/"Comment"; a flow/inline construct excludes
  headers and comments) instead of silently reusing the sibling's type or a
  hard-coded scalar. Selecting an option seeds that type's own default literal
  (`0`, `false`, a datetime stub, …) rather than the old blanket empty string,
  then proceeds into the existing inline-edit/rename flow unchanged. Keyboard
  nav mirrors the schema-enum picker exactly (↑↓/jk, Home/End, PgUp/PgDn,
  Enter, Esc) on every host; Esc now cancels before anything is inserted
  (previously the placeholder was inserted then rolled back). Also fixes a
  latent bug found while building this: adding a child directly into an
  `[[array-of-tables]]` group prepended the new `[[…]]` entry instead of
  appending it (the section-ordering clamp didn't exempt AoT parents).
  New core module `session/add_picker.rs`; new TUI overlay
  `overlay_add_picker.rs`; new web module `add-picker-items.ts`.

### Unreleased Update — 2026-09-01T05:20:00Z
- **fix(desktop): restore HTML5 drag-and-drop in the Tauri shell.** Node
  grip-drag was dead in the packaged desktop app on both Windows and macOS —
  dragged rows greyed out but the cursor showed the forbidden sign and no
  drop target reacted, while the web UI was unaffected. Root cause: Tauri v2's
  `dragDropEnabled` window option defaults to `true`, which installs an
  OS-level file-drop handler that swallows every drag session before the
  webview sees it (wry's macOS handler returns `Copy` without forwarding to
  WKWebView when the Tauri handler consumes the event; WebView2 behaves the
  same). The app never used native file drops, so the option is now
  `false` in `tauri.conf.json`, letting `web/dnd.ts` receive
  `dragover`/`drop` again.

### Unreleased Update — 2026-09-01T03:05:09Z
- **ci(msstore): revert to auto-committing the Store submission.** The
  `--noCommit` experiment (v0.31.0, v0.31.1) silently stopped shipping new
  packages: `msstore publish --noCommit` uploads the zip to an Azure blob but
  the Store only ingests it on commit, so the submission stayed at
  `PendingCommit` and Partner Center kept showing the packages cloned from the
  last published submission (v0.30.1.0, "Unchanged") — submitting that draft
  by hand re-published the old package. `publish-msstore.yml` is restored to
  plain `msstore publish` (commit == submit for certification) plus the
  RELEASES.md version sync; the human gate stays at the
  `publish-gate-msstore` environment approval. `STORE.md` documents the
  failure mode so `--noCommit` is not reintroduced.

## [v0.31.1] - 2026-09-01
### Unreleased Update — 2026-09-01T01:54:19Z
- **fix(touch): wide-layout detail panel still scrolled to top on nudge.**
  The previous fix saved/restored the wrong element's `scrollTop`: on wide
  layouts `.dp-body` is a plain padded div with no overflow of its own — the
  actual scrolling element is its parent, `.detail-pane`. `renderDetailBody()`
  (`web/touch/app.ts`) now takes an explicit `scroller` argument separate
  from the container being replaced (`.detail-pane` for wide, `.detail-wrap`
  itself for the narrow sheet, since there it doubles as `.sheet-body`).
  Verified with a real headless-Chromium repro: scrolled the wide panel
  partway down, wheel-nudged the value six times — scroll position stayed
  pinned and the value incremented on every tick.

### Unreleased Update — 2026-09-01T01:43:13Z
- **fix(touch): detail panel now live-updates during a value nudge.** Two bugs
  reported after the swipe-to-nudge feature: (1) on narrow layouts, the
  bottom-sheet detail panel only rendered once at open time — a nudge mutated
  the value but the sheet kept showing the pre-drag number until it was
  closed; (2) on wide layouts, the persistent side-pane detail panel already
  live-updated correctly, but its own scroll position (unlike the tree
  pane's, which was already preserved) snapped back to the top on every
  nudge step. Both share one fix: a new `renderDetailBody()` helper
  (`web/touch/app.ts`) re-renders whichever container is currently showing
  the panel (`.dp-body` on wide, the open sheet's `.detail-wrap` on narrow)
  on every `render()` pass, saving/restoring that container's own
  `scrollTop` around the rebuild.

### Unreleased Update — 2026-09-01T01:13:02Z
- **feat(touch): swipe-to-nudge on Integer/Float value fields.** A horizontal
  drag over an unfocused numeric value field in the detail panel now dispatches
  the same `Nudge` intent as the desktop mouse-wheel and TUI `←/→` (24px of
  drag per step, 8px dead zone before engaging, vertical scroll passes through
  via `touch-action: pan-y`). Bool is excluded (dedicated true/false picker
  sheet); tapping into the field first still gives native text
  selection/caret behavior, untouched. `web/panel.ts` only (shared by both
  desktop and touch hosts), gated on `pointerType === "touch"` so desktop
  drag-to-select-text is unaffected.


## [v0.31.0] - 2026-08-31

### Added
- `bool` scalars edit via a two-option `true`/`false` picker (schema-enum widget) on every
  platform — TUI, web/desktop, touch — with schema `enum` still taking precedence and authored
  `True`/`TRUE` casing preserved.
- Per-character highlight of fuzzy-filter matches in the filter results, including the VALUE
  cell (web + TUI).

### Fixed
- CLI tests pin `--lang en` where they assert English text; formatting drift cleaned up.

### Docs
- README / reference docs updated for the bool picker (`e` key behavior).
- RELEASES.md entries synced to v0.30.1; release version-sync requirement documented in
  CLAUDE.md; MS Store Partner Center listing export tracked.


## [v0.30.1] - 2026-08-31
### Unreleased Update — 2026-08-31T15:00:00Z
- **chore(vscode): resolve 8 pre-existing Dependabot alerts (4 high, 4 moderate)
  via `npm audit fix`.** All were transitive `devDependencies` of `@vscode/vsce`
  in `editors/vscode/package-lock.json`: `js-yaml` 4.3.0→4.3.2 (GHSA-5p4m-2wfm-xmqj,
  high), `fast-uri` 3.1.3→3.1.6 (GHSA-v2hh-gcrm-f6hx, GHSA-7p8r-x3mc-p8w7, both
  high), and `undici` 7.28.0→7.29.0 (GHSA-4cwx-7wf7-3272 high;
  GHSA-m8rv-5g2x-5cg5, GHSA-jr45-8vmc-qm54, GHSA-v3r7-h72x-cjcm,
  GHSA-8xcm-r25x-g524 moderate). Also swept up `brace-expansion` 5.0.7→5.0.9
  (GHSA-mh99-v99m-4gvg, GHSA-rgw5-rvv9-x895, high), flagged by `npm audit` but
  not yet posted to Dependabot. Every dependent's declared semver range already
  permitted the patched versions, so only `package-lock.json` changed (no
  `--force`, no `package.json` edits, no direct dependency bumps); `npm audit`
  now reports 0 vulnerabilities. Verified: `npm run check` (tsc, clean),
  `npm run build` (clean bundle), `npm test` (28/28 pass).

### Unreleased Update — 2026-08-31T14:00:00Z
- **ci(msstore): stop auto-submitting to the Store — leave the submission as a
  reviewable draft.** `publish-msstore.yml` now runs `msstore publish
  --noCommit`, which uploads the package and prepares the Partner Center
  submission but no longer commits it for certification. A human now reviews
  (and can edit) the listing in Partner Center and clicks Submit there.
  Dropped the workflow's automatic `RELEASES.md` "Current version" sync
  (`contents: write` permission removed with it) since the app isn't
  actually live until that manual submission completes. Updated
  `crates/confy-tauri/msix/STORE.md`'s *Per-release submission* section to
  match.

### Unreleased Update — 2026-08-31T13:00:00Z
- **docs(msstore): archive the v0.30.0 Partner Center listing export.** Moved the
  ad hoc `listingData-9PLCJGQ3C654-1152921505701773532.csv` (all-locale
  Description/ReleaseNotes text + screenshot references for the latest Store
  submission) into `crates/confy-tauri/msix/listings/`, prefixed with its
  version tag. Documented the archive convention in
  `crates/confy-tauri/msix/STORE.md`'s *Store listings* step: export via
  Partner Center's "Export listings" button after any manual listing edit,
  file under `listings/` — a point-in-time record only, never read by CI.

### Unreleased Update — 2026-08-31T12:00:00Z
- **fix(web): stop the armed-paste toast from replaying on every click/nav, and give
  the confirmed paste target its own visual layer.** Desktop's `renderNotice` unconditionally
  replayed the "cut N node(s)" toast's enter animation/timer on every dispatch while
  armed, including plain navigation intents (cursor move, click, `SetPasteSlot`) that
  core's Notice lifecycle deliberately leaves untouched — ported touch's `lastNoticeKey`
  fingerprint guard so the toast only re-plays when the notice text/severity actually
  changes. Separately, the committed paste target and the live mouse-hover preview
  shared the exact same DOM elements/classes (`renderPasteSlotCue`), so confirming a
  click landed correctly required moving the mouse fully off the tree; split into
  `renderConfirmedPasteCue` (always reflects `snap.paste_slot`, solid `.paste-target`/
  `#pasteTargetLine`, untouched by pointer movement) and `renderHoverCue` (client-only
  preview, dashed/muted while armed, clears fully on `mouseleave`). See
  `docs/reference/ROW_STATE_MODEL.md` §6a and §8 for the row-state model this
  participates in.

## [v0.30.0] - 2026-08-31
### Unreleased Update — 2026-08-31T00:00:00Z
- **fix(msstore): make MSIX `appExecutionAlias` schema-valid for `makeappx`.**
  Local `publish-msstore` dry-run packaging exposed that `AppxManifest.xml`
  declared `windows.appExecutionAlias` under `uap`/`uap5`, which the Windows
  SDK schema rejects. Switched to the supported namespace shape
  (`uap3:Extension` + `uap3:AppExecutionAlias` + `desktop:ExecutionAlias`,
  with `desktop`/`uap3` added to `IgnorableNamespaces`), and re-verified
  `crates/confy-tauri/msix/pack-msix.ps1` now packs
  `confy-desktop-windows-x86_64.msix` successfully.

### Unreleased Update — 2026-08-30T14:40:00Z
- **feat(msstore): bundle the TUI (`confy.exe`) into the Windows Store `.msix`
  via an App Execution Alias.** The desktop Windows release job now also
  builds `confy-tui` with the same AV-friendly profile and passes it to
  `pack-msix.ps1` (`-CliExe`), which stages it as `confy.exe`;
  `AppxManifest.xml` gains a `windows.appExecutionAlias` (`uap5`), so after
  Store/sideload install `confy` resolves on PATH through
  `%LOCALAPPDATA%\Microsoft\WindowsApps`. GitHub-Release channel unchanged.

### Unreleased Update — 2026-08-30T14:05:00Z
- **feat(tui,web): Action menu nav keys — Home/End/PageUp/PageDown alongside
  the arrows.** Core's `action_menu_move` already wraps and skips disabled
  items, but only via host-sent deltas, so each host now maps the full nav
  set: Up/Down ±1 (existing), Home/End jump to the first/last *enabled* item
  and PageUp/PageDown stride 5 (`ACTION_MENU_PAGE_STEP`, SchemaEnum's page
  convention). Because the core move strides by `delta` modulo the item
  count, a SchemaEnum-style `±len` Home/End would wrap back to a no-op — the
  hosts compute the exact `target − cursor` offset instead (web's
  `actionMenuEdgeDelta`, TUI's `App::action_menu_jump_edge`), which the
  stride loop reaches in one hop. Wired in `web/key-intent.ts` (shared by the
  desktop popover and touch external keyboards via `resolveKeyIntent`,
  `preventDefault` on the jump keys so the page never scrolls) and the TUI's
  Action-menu modal block (`crates/confy-tui/src/tui/mod.rs`); documented in
  `docs/reference/TUI.md`; 7 new `web/key-intent.spec.mjs` checks.

### Unreleased Update — 2026-08-30T13:20:00Z
- **fix(core,tui,web): Action menu follow-ups — TUI external edit, touch Detail
  routing, touch FAB position/icon, dimmed disabled items.**
  Four defects found while exercising the new Action menu (commit `3ebb397`):
  (1) TUI's Action-menu `Edit` item went through core's `begin_external_edit`
  handshake, which only *records* a pending edit — it never spawned `$EDITOR`
  (that shape exists for Web's async round-trip). `action_menu_commit()` now
  drains `pending_external_edit` and performs the same synchronous
  spawn-and-apply the direct `e` key uses. (2) Touch's Action-menu `Detail`
  item sent core's `ToggleDetail` intent, but touch's detail sheet is
  deliberately host-local (`i`/Enter bypass core for it), so nothing happened;
  the item now exits the menu and calls `toggleDetailSheet()` directly.
  (3) Touch's FAB was a sibling of `.statusbar` under `.app` (full-screen
  `position:absolute`), so `bottom:18px` measured from the screen edge and the
  button sat behind the status bar; it now nests inside `.body`
  (`position:relative`, already above the status bar), mirroring desktop's
  `.main` fix. (4) The FAB glyph changed from `+` to a vertical three-dot
  "actions" icon (`FAB_PLUS_IC` → `FAB_ACTIONS_IC`) — "+" implied create-only;
  and touch's stylesheet gained the missing `.menu-item:disabled{opacity:.35}`
  rule so unsupported items (e.g. Add child on scalars/comments) dim like
  desktop and the TUI already did.

### Unreleased Update — 2026-08-30T12:30:00Z
- **docs: full documentation audit — sync all reference/root docs with the code.**
  Eight parallel verification passes cross-checked every committed doc against the
  working tree; ~40 wrong/outdated claims fixed in place, nothing deleted (the
  `docs/superpowers/` frozen-history policy and the ADR record are preserved
  untouched except for status lines). Action-menu fallout (commit `3ebb397`) synced
  into `README.md` (keybindings: `m` is now the Action menu, not Move; added the
  missing `1`/`2`, `f`, `F2`, `~` rows), `WEBUI.md` (ModeView gains
  `ActionMenu`/`SchemaEnum`; row anatomy is grip-only; FAB opens the Action menu /
  pastes when armed; double-click toggles Detail; right-click opens the Action menu;
  prompt question comes from `snap.mode.Prompt.question`; shared panel is
  editing-only, `afterMutation` gone; touch panel/FAB updated; `m` added to the
  external-keyboard list), and `ROW_STATE_MODEL.md` (Touch's selection row, Action
  menu in the modal-lock list, refreshed `session.rs` guard line refs, §2/§8
  cross-reference fixes). State-lift renames applied in `TUI.md` (`App.*` fields are
  `Session.*` since the headless-core lift) and CLAUDE.md (filter haystack now
  includes scalar values; `l` picker key case). `MESSAGES.md` severity count
  corrected to 42 `core.*` keys (11E + 14W + 7S + 9I, incl. `core.action.unavailable`).
  `VSCODE.md` gained the missing `convert-save` protocol row and dropped the stale
  "(0.2.1)" header tag; `editors/vscode/README.md` names the real
  `publish-gate-vscode` environment and the Theme submenu. `TAURI.md`: About is a
  custom MenuItem (only Quit is Predefined), the Edit menu has no Predefined
  text-edit items, `openTauriPath` goes through `tauri-plugin-fs`'s
  `fs.readTextFile`, and the mobile no-op guard no longer claims a `canSaveAs()`
  link (M2 made it unconditional). `BEHAVIOR_MATRIX.md` backend splice paths fixed to
  the `cst_edit/`/`yaml/edit/` module dirs; `CONTEXT.md` Type-filter glossary now
  covers Flags + Reverse and points at `docs/reference/BEHAVIOR_MATRIX.md` (not
  "repo root"); `PORTING.md` gets an explicit "port is COMPLETE" status banner;
  ADR 0009 + `docs/adr/README.md` marked Implemented (2026-08-30) instead of
  "implementation pending". Also: 25 post-tag "Unreleased Update" entries
  (2026-08-29T14:34Z → 2026-08-30T05:30Z) that had been misfiled under
  `## [v0.23.0]` moved up into `## [Unreleased]` (verified against
  `git show v0.23.0:CHANGELOG.md`; tag was cut at 13:28Z), and `web/cf-build.sh`'s
  header comment says Workers Builds, not Pages. Docs-only change — no code touched;
  verified with a repo-wide stale-claim sweep and a relative-link check (all resolve).

### Unreleased Update — 2026-08-30T11:24:59Z
- **feat(core,tui,web): centralized Action menu — one core-owned node-operation
  menu replacing five disagreeing surfaces.** Implements the approved design
  (`docs/superpowers/specs/2026-08-30-action-menu-design.md`, ADR 0009).
  `confy-core` gains `Mode::ActionMenu`/`ModeView::ActionMenu`
  (`crates/confy-core/src/session/action_menu.rs`, new), an eight-item list
  (Edit in editor, Add child, Add sibling, Copy, Cut, Toggle comment, Detail,
  Delete) computed fresh from `selected_paths()` every snapshot, and five new
  `Intent`s (`OpenActionMenu`/`ActionMenuMove`/`ActionMenuCommit`/
  `ActionMenuPick`/`ExitActionMenu`). Added 11 `core.action.*` i18n keys to
  both catalogs. The TUI gets a new `m` overlay
  (`crates/confy-tui/src/tui/overlay_action_menu.rs`, new) mirroring
  `overlay_kind_switch.rs`'s shape, plus four thin `App` proxies. The desktop
  web UI drops the per-row `⋮` context menu (`buildCtxMenu`/`openCtxMenuAt`)
  and the FAB's context-add decision (`fabAddAction`, deleted) in favor of a
  shared `buildActionMenu`/`openActionMenuAt`, reachable via the row grip's
  right-click, the Action button (`data-act="actions"`, was `add`), and the
  new `m` key; the floating button/paste-clear pair moved from
  `position:fixed` on `<body>` to `position:absolute` inside `.main` so it
  can never overlap the footer/status bar and stays clear of the open detail
  panel. Touch gets a new bottom sheet (`openActionMenuSheet`, driven by
  `snap.mode` like the existing TypeFilter/Convert/Prompt sheets) and the
  same Action-button retarget; `addContextual`/`fabAddAction` removed as
  orphaned by the FAB's new open-menu behavior. The detail panel
  (`web/panel.ts`) loses its four `.row-btns` action buttons (Edit/Copy/Cut/
  Delete) on both hosts — every editing affordance (rename, value edit,
  multi-line edit, schema enum select, trailing comment, comment-node edit,
  kind badge) is unchanged — and `wirePanel`'s now-unused `afterMutation`
  parameter was dropped, updating all three call sites. Item rendering is
  shared between desktop and touch via `web/action-menu-items.ts` (new),
  matching the Tauri native Edit menu's Copy/Cut/Paste Node items excluded
  from this unification (OS-convention chrome, per ADR 0009 §9). Fixed two
  tests left stale by the panel/FAB changes:
  `web/panel-schema.spec.mjs`'s ordering assertion (Schema no longer precedes
  a `row-btns` block that doesn't exist) and
  `web/touch-modal-lock.spec.mjs`'s FAB test (`data-act="add"` →
  `"actions"`). Verified: `cargo test -p confy-core` (all tests pass),
  `cargo build --workspace` (clean), `web` `tsc --noEmit` (clean),
  `web/build.mjs` (clean bundle), and the full `web` test suite (all specs
  pass, including `fab.spec.mjs`, `panel-schema.spec.mjs`, and
  `touch-modal-lock.spec.mjs` after the two fixes above).

### Unreleased Update — 2026-08-30T10:48:27Z
- **docs(design): grill the Action menu design; ADR 0009 + CONTEXT.md terms.** A
  question-driven review pass over the design spec added
  `docs/adr/0009-centralized-action-menu-core-owned.md`, four glossary entries
  to `docs/reference/CONTEXT.md` (**Overflow menu**, **Action menu**, **Action
  button**, **Native menu bar**) plus a line on **Remark** recording that its
  user-facing label is "Toggle comment" on every host, and rewrote the spec
  around evidence. Two claims in the first draft were verified false and are
  now corrected: (1) the desktop detail panel does **not** stay open when the
  Action menu opens — `Mode` is a single-slot enum with no mode stack
  (`state.rs:44-63`) and the desktop panel is `Mode::Detail`-driven
  (`ui.ts:543`), so both web hosts close it; accepted because the panel's own
  kind badge already does exactly this via `Mode::KindSwitch`
  (`ui.ts:585-594`). (2) `ExternalEditKind::Comment` (`view.rs:227-232`) edits a
  **comment node's text** via `ApplyEditComment`, not a trailing comment, so the
  claimed TUI route for "Append comment" never existed — and the TUI has no
  trailing-comment creation path at all (`app.rs:589-653`). The item model
  shrank from 10 to **8**, each exactly one core intent with no host-mapped
  exceptions: Paste was already unreachable (opening is refused while
  Clipboard-armed, so today's `⋮` Paste entry is dead code), Append comment was
  dropped (both web hosts already create/change/clear a trailing comment through
  the panel input `panel.ts:132-143`, which is kept), and Kind switch was
  dropped because the node carries a dedicated always-visible control — the kind
  badge (`render.ts:90-94`, routed at `ui.ts:1254`). That exclusion is now a
  written membership rule rather than a case-by-case judgement: an operation
  belongs to the Action menu when core can express it as a single intent over
  the target set, unless the node already has a dedicated always-visible control
  for it; in-place text entry belongs to the detail panel. Eligibility likewise
  became derived rather than enumerated — an item is single-node-only exactly
  when the core state behind it carries one `Path` — which yields 4 of 8 dimmed
  on a multi-node selection and 7 of 8 when the selection contains a read-only
  node, the reason ineligible items are shown disabled rather than hidden. Also
  resolved: six section headers replaced by one `separator_before` flag above
  Delete (i18n keys down from 19 to 11), a core-supplied `target_label` naming
  the node when a single one is targeted (the menu no longer opens *at* the
  row), `ActionMenuCommit`/`Pick` exit to `resting_mode()` **before**
  dispatching, desktop gains `m` for key parity with the TUI, the `Edit` item is
  labeled "Edit in editor" since it dispatches `BeginEditExternal`, and the
  Tauri native Edit menu's Copy/Cut/Paste Node items (`web/menu.ts:333-347` — a
  fifth node-op surface the first draft never counted) are documented as an
  exempt OS-convention surface. Recorded a silent-failure hazard for the
  implementation: `wirePanel` is positional, so dropping its `afterMutation`
  parameter shifts trailing args at all three call sites and would break the
  panel's kind button or schema `<select>` without a compile error. No code
  changes yet.

### Unreleased Update — 2026-08-30T09:54:26Z
- **docs(design): Action menu — centralized node operations across desktop,
  touch, and TUI.** Approved design spec for replacing the per-row desktop `⋮`
  menu (`web/render.ts` + `buildCtxMenu` in `web/ui.ts`), the floating `+`
  (`web/fab.ts`), and the detail panel's four action buttons (`web/panel.ts`)
  with one **Action menu** whose item model, eligibility, and open state are
  owned by `confy-core` as a new `ModeView::ActionMenu` variant and rendered
  three ways: desktop popup, touch bottom sheet, and a new TUI overlay on `m`.
  Establishes the terminology split the codebase was missing — **Overflow
  menu** (the RWD-folded toolbar menu derived from `foldedEntries`) vs
  **Action menu** vs **Action button** — resolving the `data-act="menu"`
  collision where the same attribute meant the overflow sheet on touch and the
  per-row node menu on desktop. Node rows keep only a move grip on both web
  surfaces; the detail panel keeps every editing affordance and loses only its
  actions; touch multi-selection gains its first usable operation surface via
  core-computed per-item eligibility over `selected_paths()`. Two findings from
  the design pass: the menu's Paste item is unreachable by construction
  (opening is refused while Clipboard-armed) and today's context-menu Paste
  entry is therefore *already* dead code; and the Action button's status-bar
  overlap is fixed structurally with a non-scrolling wrapper around the tree
  scroller rather than a hardcoded offset, which also keeps the button clear of
  the open detail panel. No code changes yet — spec only, at
  `docs/superpowers/specs/2026-08-30-action-menu-design.md`; implementation
  plan next.

### Unreleased Update — 2026-08-30T06:05:13Z
- **feat(web): touch UI parity — comment-advisory card, wavy underline,
  swipe-to-remark.** Three touch-UI gaps vs. desktop, closed: (1) the detail
  panel's `comment-advisory` note now gets a warn-bordered card
  (`.detail .comment-advisory` in `web/touch/style.css`, matching
  `.detail .schema-info`'s box language) instead of rendering as bare text —
  `web/panel.ts` already emitted the markup, touch just had no CSS for it.
  (2) Tree rows with `ViewRow.comment_advisory` set now get desktop's wavy
  warning underline on the comment/trailing-comment span
  (`.comment.comment-advisory` in `web/touch/render.ts` +
  `web/touch/style.css`); desktop's own `.comment-advisory` selector
  (`web/style.css`) is now scoped to `.comment.comment-advisory` so the rule
  no longer also (accidentally) applies inside the desktop detail panel's
  advisory card, which never had a text-decoration reset. (3) Touch gains a
  right-swipe gesture revealing a neutral `.row-remark` button (mirrors the
  existing left-swipe `.row-del`), dispatching `Intent::Remark` — the
  desktop-only `r` key / "Toggle comment" menu item was previously
  unreachable on touch. `web/touch/app.ts`'s swipe state generalized from
  one-sided (`-SWIPE_W`..`0`) to bidirectional (`-SWIPE_W`..`SWIPE_W`,
  clamped per-row to whichever actions the row actually carries);
  `setDelRevealed` renamed `setSwipeRevealed` (not action-specific). Fixed a
  latent tap-routing gap while implementing this: `pointerdown`'s
  `.row-main`/`.row-del` `closest()` fallback for an already-swiped-open row
  now also checks `.row-remark`, or a tap on the revealed remark button
  would resolve to no row. Added `web/touch-comment-advisory.spec.mjs`;
  synced the renamed/extended swipe state into
  `web/touch-modal-lock.spec.mjs` and `web/touch-paste-drag.spec.mjs`,
  which extract `installTreeGestures` verbatim from `touch/app.ts`. Verified
  end-to-end: `crates/confy-ffi` rebuilt via `wasm-pack`, `web/build.mjs`
  bundled cleanly, the dev server served the built `touch/app.js` and
  `touch/style.css`/`style.css` containing the new markup/rules, `tsc
  --noEmit` and the full `web` test suite (existing + new) pass, and
  `cargo test -p confy-core` passes (no Rust files touched).

### Unreleased Update — 2026-08-30T05:30:54Z
- **fix(core): map `tui.lang.saved` in `severity_of` to stop a TUI
  language-picker panic.** Applying a language via the TUI's language
  picker (`l`, select, `Enter`) crashed: `lang_picker_commit` dispatches
  `Intent::SetHostNotice { key: "tui.lang.saved", .. }` after a successful
  config save, but `session::notice::severity_of`'s match table had no
  arm for that key, so it fell through to the fallback
  `panic!("severity_of: unmapped notice key ...")`. The i18n catalogs
  (`i18n/en.json`, `i18n/zh-TW.json`) already had correct text for the
  key; only the severity mapping was missing. Added `"tui.lang.saved"`
  to the `Severity::Success` arm, alongside its existing sibling
  `"tui.host.saved"` and pairing with `"tui.lang.save-failed"`'s
  existing `Severity::Error`. Added a regression test,
  `set_host_notice_tui_lang_saved_does_not_panic`, to
  `session_snapshot_notice.rs`, confirmed it panicked against the
  unfixed table before the fix and passes after. Manually reproduced
  and re-verified on the real `confy` binary (isolated
  `XDG_CONFIG_HOME` scratch dir): before the fix, selecting 繁體中文
  (zh-TW) in the language picker crashed the process; after the fix,
  the status line shows "語言已設定為 zh-TW" and the app keeps running.
  Found during an earlier audit follow-up's manual verification but
  deliberately deferred as out of that batch's scope; fixed here at the
  user's explicit request to resolve the noted ambiguity.

### Unreleased Update — 2026-08-30T05:30:00Z
- **refactor(core): deduplicate the 5 collision-suffix-loop implementations
  into `node::next_available_key`.** Audit follow-up (batch 4). All 5
  `OnCollision::Rename` handlers (`json/edit.rs`, `yaml/edit/block.rs`,
  `yaml/edit/flow.rs`, `cst_edit/move_paste.rs`, `cst_edit/aot_group.rs`)
  independently reimplemented the same `key_2`, `key_3`, … candidate
  search. Added `pub fn next_available_key(base: &str, is_taken: impl
  Fn(&str) -> bool) -> String` to `model::node` and converted all 5 call
  sites to it, each keeping its own existing "taken" predicate (`Vec`
  scan for JSON/YAML, tree lookup via `node_at` for the two TOML CST
  sites) and its own post-selection logic (comment/value rebuilding)
  unchanged.

  `cst_edit/move_paste.rs`'s call site needed a different closure shape
  than originally planned: a closure that mutates its captured `segs` to
  build each candidate path would need `FnMut`, but
  `next_available_key`'s `is_taken` parameter is a plain `Fn` — so used a
  clone-based closure instead (builds a fresh candidate `Vec` per check,
  same idiom already used at the `aot_group.rs` site), which compiles
  and is not fragile.

  Verified: `cargo build -p confy-core` compiles clean at all 5 sites;
  `cargo test -p confy-core --lib` — the full 35-test
  `rename`/`collision`-filtered run and the full 550-test suite — both
  pass at baseline, confirming `next_available_key`'s extracted logic
  produces byte-identical output to the 5 original inline loops;
  clippy/fmt clean.

### Unreleased Update — 2026-08-30T05:15:00Z
- **refactor(core): add `ConfigDocument` trait defaults for `to_value` and
  `serialize_fragment_relative`.** Audit follow-up (batch 4). All three
  backends' `to_value` implementations were identical except for the
  `DocFormat` literal passed to `tree_to_value` — and the trait already
  has `fn format(&self) -> DocFormat` each backend implements to return
  exactly that literal, so gave the trait a default using `self.format()`
  and deleted the three now-redundant overrides
  (`cst_doc.rs`/`json/doc.rs`/`yaml/doc.rs`). For
  `serialize_fragment_relative`, JSON and YAML's overrides were both
  exactly `self.serialize_fragment(path)` ("no dotted scope tables, so
  relative == absolute fragment") — gave the trait that same default and
  deleted those two overrides; TOML's override differs (dotted-scope
  tables need `cst_edit::serialize_fragment_relative`) and was kept
  unchanged. Verified: `cargo build --workspace` compiles clean with all
  three backends satisfying `ConfigDocument` via the new defaults;
  `cargo test -p confy-core --lib` holds at the 550-test baseline;
  clippy/fmt clean.

### Unreleased Update — 2026-08-30T05:05:00Z
- **refactor(tui): extract `app.rs`'s inline tests to a sibling `tests.rs`.**
  Audit follow-up (batch 4), the third and last instance of the
  `#[path = "tests.rs"] mod tests;` convention batch 3 introduced for
  `confy-core`'s `cst_edit/mod.rs` and `yaml/edit/mod.rs`.
  `crates/confy-tui/src/tui/app.rs` was 4,446 lines, with a single
  `#[cfg(test)] mod tests { ... }` block spanning lines 921-4446. The
  tests called exactly one private item outside the module — `fn
  type_tag` — so widened it to `pub(crate) fn type_tag` (no signature
  change) and moved the test module body to a new
  `crates/confy-tui/src/tui/tests.rs`, leaving
  `#[cfg(test)] #[path = "tests.rs"] mod tests;` in `app.rs`.
  `app.rs` is now 923 lines (production code only); `tests.rs` holds
  the moved tests. Verified: `cargo build -p confy-tui` compiles clean;
  `cargo test -p confy-tui --lib` holds at the pre-move 210-test
  baseline; clippy/fmt clean.

### Unreleased Update — 2026-08-30T04:56:00Z
- **chore(deps): upgrade `unicode-width` from 0.1 to 0.2 in `confy-tui`.**
  Audit follow-up (batch 4). `UnicodeWidthStr`/`UnicodeWidthChar` trait
  method signatures (`.width()`) are unchanged between the two majors;
  the only 3 call sites (`tui/overlay_lang_picker.rs`, `tui/ui.rs`)
  measure single-line UI label/title-bar strings with no embedded
  newlines, so 0.2's changed `\n`-width behavior doesn't apply, and CJK
  Unified Ideograph/Fullwidth-form widths (what this crate depends on
  for "CJK-safe alignment") are unchanged between versions. `ratatui`
  0.28's own dependency chain (`unicode-truncate`) still pulls
  `unicode-width` 0.1.14 internally, so both majors resolve side by
  side — expected and harmless; `confy-tui` itself resolves 0.2.2
  directly. Verified: `cargo build -p confy-tui` compiles clean with
  zero source edits; `cargo test -p confy-tui --lib` holds at its
  210-test baseline; clippy/fmt clean. Manually verified on the real
  `confy` binary (`--lang zh-TW`): both the title bar's right-aligned
  version number and the language-picker popup's border alignment
  render correctly with the Traditional Chinese labels "名稱"/"數值"/
  "語言"/"繁體中文 (zh-TW)" visible.

  While verifying, found (but did not fix — out of this batch's scope)
  a pre-existing, unrelated bug: applying a language via the picker's
  Enter key panics at `confy-core/src/session/notice.rs:126`
  (`severity_of: unmapped notice key "tui.lang.saved"`) because
  `tui.lang.saved` (used at `confy-tui/src/tui/app.rs:544`, present in
  both `i18n/en.json` and `i18n/zh-TW.json`) was never added to
  `severity_of`'s match table.


### Unreleased Update — 2026-08-30T04:41:18Z
- **chore(deps): upgrade `thiserror` from 1 to 2.**
  Audit follow-up (batch 4). Confirmed via thiserror's 2.0.0 release notes
  that none of its breaking changes apply here: all 4
  `#[derive(thiserror::Error)]` usages (`confy-core`'s `ConvertAbort`,
  `MutateError`, `ParseError`; `tauri-plugin-confy-picker`'s `Error`) use
  only plain `#[error("...")]` string literals and
  `#[error(transparent)]` + `#[from]`, none of the attribute/API surface
  2.0 changed. Bumped `thiserror = "1"` to `"2"` in the workspace
  `[workspace.dependencies]`. `taplo` (an existing transitive dependency,
  already documented as unmaintained in this changelog) and its
  `json-patch`/tauri chain still pull `thiserror` 1.0.69 internally, so
  `cargo tree` now shows both major versions resolved side by side — this
  is expected and harmless (Cargo compiles distinct majors independently);
  `confy-core`, `confy-tui`, and `tauri-plugin-confy-picker` all resolve
  `thiserror` 2.0.20 directly, confirmed via `cargo tree -i thiserror@2.0.20`.
  Verified: `cargo build --workspace` compiles clean with zero source
  edits beyond the one `Cargo.toml` version bump; `cargo test -p
  confy-core --lib` (550 passed) and `cargo test -p confy-tui --lib` (210
  passed) both hold at their pre-bump baselines; clippy/fmt clean.


### Unreleased Update — 2026-08-30T04:10:00Z
- **chore(tui): dedupe the `dirs` dependency to a single resolved version.**
  `confy-tui` declared `dirs = "5"` while `tauri`'s own dependency chain
  pulls `dirs` 6.0.0, so the build resolved and compiled both major
  versions side by side. `dirs::config_dir()`/`home_dir()` (the only two
  calls, `confy-tui/src/config.rs`) have no signature/behavior change
  between 5.x and 6.x, so bumped `confy-tui`'s constraint to `dirs = "6"`.
  Verified: `cargo tree -i dirs` now shows a single `dirs v6.0.0` node;
  `cargo build -p confy-tui` compiles clean; `cargo test -p confy-tui`
  passes at its existing baseline; clippy/fmt clean.

### Unreleased Update — 2026-08-30T04:00:00Z
- **test(core): add a fixture-seeded round-trip property test per backend.**
  Added `proptest = "1"` (`confy-core`'s dev-dependencies were empty until
  now) and `tests/roundtrip_proptest.rs`: one `proptest!` fn per backend
  (`toml_fixture_roundtrips`/`json_fixture_roundtrips`/
  `yaml_fixture_roundtrips`), each sampling (`prop::sample::select`) over
  the same curated fixture set already on disk that
  `roundtrip.rs`/`roundtrip_json.rs`/`roundtrip_yaml.rs` already trust —
  no synthetic config-syntax generator, so proptest's harness
  (shrinking/seed-reporting on failure) runs over a corpus already known to
  be format-valid rather than fighting each format's grammar. Cross-checked
  the fixture lists against the existing round-trip tests' own
  enumerations before finalizing: dropped `yaml/multi-doc.yaml` (already
  documented in `roundtrip_yaml.rs` as intentionally excluded — rejected at
  parse, not a round-trip candidate). All listed fixtures verified to
  actually round-trip by running the new tests directly (all 3 pass at the
  default 256 cases each, no `proptest-regressions` file produced).
  `cargo test --workspace` stays at the 550-test confy-core baseline plus
  3 new passing tests; clippy/fmt clean.

### Unreleased Update — 2026-08-30T03:45:00Z
- **test(core): convert two 3-way format-parity test files to table-driven
  loops.** `tests/external_edit_clears_trailing_comment.rs` and
  `tests/insert_after_trailing_comment.rs` each had 3 near-identical
  per-format `#[test]` fns differing only in fixture strings/`DocFormat`/
  assertion specifics — the shape the 2026-08-29 audit's "table-driven
  parity test" P2 finding was about. `external_edit_can_clear_trailing_comment`
  now loops JSON+TOML (exact-string assertions) and asserts YAML separately
  (its own pre-existing "comment absent" negation, not an exact string, since
  the CST doesn't guarantee identical post-splice whitespace there).
  `toml_and_yaml_keep_trailing_comment_attached` loops TOML+YAML (identical
  "find the line, assert it still carries the comment" shape); JSON's
  `json_add_sibling_keeps_trailing_comment_attached` stays a standalone
  `#[test]` since it pins a JSON-specific regression with an exact
  full-string `assert_eq!`, not the same assertion shape. Same inputs, same
  expected outputs, no behavior change to the code under test. Verified
  each loop actually exercises every format by temporarily corrupting one
  iteration's expected value in each file and confirming the test fails
  (then reverting); `cargo test --workspace` stays at the 550-test
  confy-core baseline; clippy/fmt clean.

### Unreleased Update — 2026-08-30T03:30:00Z
- **refactor(core): extract two 80%+-inline-test modules to sibling
  `tests.rs` files.** `model/cst_edit/mod.rs` (3,383 lines, 3,104 of them
  `#[cfg(test)] mod tests { ... }`) and `model/yaml/edit/mod.rs` (2,034
  lines, 1,877 of them tests) buried their small runtime bodies (279 and
  157 lines respectively) under a much larger inline test module. Moved
  each test module's body verbatim into a sibling `tests.rs`
  (`#[cfg(test)] #[path = "tests.rs"] mod tests;` stub left in `mod.rs`),
  matching the standard Rust idiom for large inline test modules; no test
  code changed, only its file location (`cargo fmt` then re-dedents the
  moved content from module-body indent to top-level). Left
  `confy-tui/src/tui/app.rs`'s inline tests (3,526 lines) alone — its test
  module calls the private fn `type_tag`, which this batch doesn't touch.
  Fallout: `tests/no_fs_gate.rs`'s PORTING.md §7 boundary-gate scan finds
  each file's "runtime" prefix by slicing at the first `#[cfg(test)]`
  token — since that marker now lives in `mod.rs`, not in the extracted
  `tests.rs`, the gate started scanning `yaml/edit/tests.rs` as if it were
  runtime code and failed on a legitimate `std::fs::read_to_string` inside
  a test fixture. Fixed by skipping any file literally named `tests.rs` in
  the gate's directory walk (documented as part of the sibling-test-file
  convention in both the scan loop and the file's top doc comment).
  Verified: `cargo test -p confy-core --lib model::cst_edit::` (194 pass)
  and `--lib model::yaml::edit::` (116 pass) individually, then
  `cargo test --workspace` at the unchanged 550-test confy-core baseline
  including `no_fs_gate` passing again; `cargo clippy --workspace
  --all-targets -- -D warnings` and `cargo fmt --all --check` both clean.

### Unreleased Update — 2026-08-30T03:15:00Z
- **ci(web): wire `crates/confy-ffi/functional_smoke.mjs` into the build
  pipeline — it was never actually running.** `web/run-tests.mjs` only
  discovers `*.spec.mjs` files inside `web/`, so this Stage-2 wasm
  functional-smoke test (wrong directory, wrong filename pattern) was
  silently skipped by both `web-ci.yml` and every real Cloudflare Pages
  deploy. Added `( cd crates/confy-ffi && node functional_smoke.mjs )` to
  `web/cf-build.sh`, right after the `wasm-pack build` step that produces
  the `pkg/` output it imports and before the `web/` typecheck+test+bundle
  step — a failure here now aborts the build the same way a typecheck or
  test regression already does (`set -euo pipefail`). Verified end-to-end:
  ran the full `bash web/cf-build.sh` locally (completes with
  `cf-build: assembled web/dist`) and separately ran
  `node functional_smoke.mjs` standalone after a fresh `wasm-pack build`,
  confirming all 111 checks pass and it prints `ALL FUNCTIONAL CHECKS
  PASSED` / exits 0 (confirmed the script's own `process.exit(failures ===
  0 ? 0 : 1)` already fails the build correctly on a real failure — no
  further fix needed there).

### Unreleased Update — 2026-08-30T03:00:00Z
- **docs: correct the record on the `quick-xml` RUSTSEC findings — already
  moot for CI, not a pending fix.** The prior entry (below) reported
  `cargo audit` flagging `quick-xml` 0.39.4 (RUSTSEC-2026-0194/0195, high
  severity, via `plist` 1.9.0) and said it "audits the checked-in
  `Cargo.lock` directly" — but `Cargo.lock` is `.gitignore`d and has never
  been committed (confirmed: `git ls-files` / `git log -- Cargo.lock` are
  both empty), so that description was wrong and the finding was an
  artifact of a stale `Cargo.lock` left on disk in this dev environment,
  not something CI's `cargo audit` step (which always resolves a fresh
  lockfile on checkout) would ever see. Verified directly: deleting
  `Cargo.lock` and running `cargo generate-lockfile` — what CI effectively
  does — resolves `plist` straight to 1.10.0 / `quick-xml` to 0.41.0,
  since every parent crate's `plist = "1"` constraint already permits it.
  No repo file changed; recorded here so the false "pending fix" isn't
  carried forward.

### Unreleased Update — 2026-08-30T02:30:00Z
- **ci(rust): add a `cargo audit` step; docs: document the `taplo`
  unmaintained-upstream risk.** `.github/workflows/rust-ci.yml` had no
  dependency-vulnerability scan. Added `cargo install cargo-audit
  --locked` + `cargo audit` as the last two steps of the existing `rust`
  job (visibility only, same non-required-check framing as the rest of
  the file — no config file needed, it audits the checked-in
  `Cargo.lock` directly). Added a `## Known Risks` section to
  `CLAUDE.md` (between `## Architecture` and `## Module map`)
  documenting why `taplo` being unmaintained upstream isn't being acted
  on now (small used surface, ~1,240 LOC vendoring estimate as the
  pre-planned contingency, `tombi` not yet a usable migration target)
  and naming the new CI step as the trigger for revisiting that
  decision. Ran `cargo audit` locally to confirm the step works: it
  currently reports 2 real advisories (both `quick-xml` 0.39.4,
  RUSTSEC-2026-0194/0195, high severity, pulled in transitively — not
  `taplo`/`rowan`) plus 19 unmaintained-crate warnings; per this batch's
  scope, reported here and left for a separate, dedicated fix rather
  than bundled into this CI/docs change.

### Unreleased Update — 2026-08-30T02:15:00Z
- **refactor(tui): route the Help-tab toggle and SchemaEnum navigation
  through `Session::apply`.** `crates/confy-tui/src/tui/mod.rs` called
  `session.toggle_help_tab()` and the six `session.schema_enum_move`/
  `schema_enum_jump`/`schema_enum_commit` methods directly, bypassing
  the `Intent`/`apply` dispatch every other keybinding in this file
  already goes through — both already have matching `Intent` variants
  (`ToggleHelpTab`, `SchemaEnumMove`/`Jump`/`Commit`) with dispatch arms
  that call the exact same methods, so this is a routing-only change,
  no behavior difference. Left two other bypass sites from the same
  audit finding untouched: `last_action_was_shift_select`'s direct
  write and `app.rs`'s `convert_write` mode assignment have no matching
  `Intent` variant today (inventing one is a separate design decision);
  and the `paste_slot` direct write, because `Session::set_paste_slot`
  (the method `Intent::SetPasteSlot` dispatches to) does more than a
  plain assignment — it also gates on the target's visibility and syncs
  `cursor` — so swapping it in here isn't a pure mechanical change.
  Verified both converted paths on the real `confy-tui` binary against
  a scratch `#:schema`-linked TOML file with an `enum`-constrained
  field: opened the Schema-value picker, moved the selection with
  Down, committed with Enter, and confirmed the saved file now reads
  `level = "info"`; separately opened Help and confirmed Tab flips the
  overlay to the About tab and back.

### Unreleased Update — 2026-08-30T02:00:00Z
- **refactor(web): move the Help overlay's KIND-badge legend into the i18n
  catalogs.** `web/help-content.ts` hard-coded `KIND_LEGEND`/
  `KIND_LEGEND_ZH_TW` (`Record<string, string>` keyed `Toml`/`Json`/`Yaml`)
  as a second, parallel translation mechanism alongside the `i18n/*.json`
  catalogs `t()`/`tArgs()` already read. Moved both records' text
  verbatim into three new flat keys per catalog —
  `web.help.legend.toml`/`.json`/`.yaml` (`i18n/en.json`,
  `i18n/zh-TW.json`) — and `helpBodyHTML` now does
  `t(\`web.help.legend.${docFormat.toLowerCase()}\`)` instead of an
  object literal lookup, going through `t()`'s existing active-lang →
  `en` → raw-key fallback chain like every other string in the app.
  Verified end-to-end with a real esbuild bundle of `help-content.ts`:
  the rendered zh-TW YAML Help body is byte-identical to the deleted
  `KIND_LEGEND_ZH_TW.Yaml` constant, including `helpLineHTML`'s
  existing per-line HTML escaping.

### Unreleased Update — 2026-08-30T01:45:00Z
- **refactor(core, web): compute the kind badge's label/note once in core;
  stop re-deriving it in three web files.** `web/kind-labels.ts`'s
  `KIND_SHORT`/`NOTATION_SHORT`/`CONTAINER_NOTE`/`notationGlyph`/
  `kindLabelParts` re-derived the same "table"/"AoT"/"str"/… friendly
  label and "·scope"/"·0x"/"·dec"/… notation note from `ViewRow.type_label`
  + `.format` independently in `render.ts`, `panel.ts`, and
  `touch/render.ts`. Added `Session::to_view_row`-computed
  `ViewRow.badge_label`/`.badge_note` (`Cow<'static, str>`, same
  `Deserialize`-derive reason as the prior `type_label`/`key_sign` change)
  via a new `status_fmt::badge_label_note`, a line-for-line Rust port of
  the deleted TS logic. All three web files now destructure
  `r.badge_label`/`r.badge_note` directly; `kind-labels.ts` keeps only
  `valueHue`/`valueTypeClass`/`isCommentRow`/`isPositional`/`isExpanded`,
  which aren't derivable from the new fields. Verified the ported logic
  against `visible_rows()` output for 9 real TOML/YAML documents
  (`[header]` scope table, dotted table, inline table, multiline array,
  float, basic string, array-of-tables, YAML block map, YAML flow map) —
  every label/note pair matches the deleted TS code's output for the
  same shape.

### Unreleased Update — 2026-08-30T01:20:00Z
- **refactor(core): replace `anyhow::Result` with a structured `ParseError`
  in the four document `from_str` constructors.** `CstDocument::from_str`,
  `JsonDocument::from_str`, `YamlDocument::from_str`, and
  `AnyDocument::from_str_as` all returned untyped `anyhow::Result<Self>`,
  so a library consumer had no way to match on "TOML/JSON/YAML parse
  failure" without downcasting a `dyn Error`. Added
  `document::ParseError` (`Toml`/`Json`/`Yaml` variants, `thiserror`,
  same message text each backend already produced) alongside the
  existing `MutateError`, and switched all four constructors to return
  it directly. Every one of the ~60 existing call sites across the
  workspace only used `?`, `.unwrap()`/`.expect()`, Display formatting,
  or `anyhow::Context::with_context` (which still works unchanged, since
  `anyhow::Context` accepts any `std::error::Error + Send + Sync +
  'static`), so only one file needed a change: a test helper in
  `tests/yaml_scratch.rs` that returned the bare `Result` instead of
  propagating it through `?`. Verified on the real `confy-tui` binary
  that a malformed TOML file's reported error text is unchanged.

### Unreleased Update — 2026-08-30T01:10:26Z
- **perf(core): `ViewRow.type_label`/`.key_sign` stop allocating a `String`
  on every visible row.** `to_view_row` called `.to_string()` on
  `node_type_label_str`/`key_sign_label`, both of which already return
  `&'static str` — every row rebuild allocated two throwaway strings it
  didn't need. Changed both fields to `Cow<'static, str>` (not a bare
  `&'static str`: `ViewRow` derives `Deserialize` for the
  `serde_json` round-trip contract test in `tests/serde_roundtrip.rs`
  (PORTING.md §7 exit gate #3), and a struct with a genuinely `'static`
  borrowed field can't implement `Deserialize<'de>` for arbitrary `'de`;
  `Cow` keeps the zero-alloc write path while still deserializing into an
  owned `String` when needed). `confy-tui`'s `RowSnapshot` — the one place
  that needs an owned `String` — now calls `.into_owned()` at that single
  host boundary instead of the allocation happening inside core for every
  row.

### Unreleased Update — 2026-08-30T00:35:09Z
- **bench(core): add a YAML `Move` case to `perf.rs` measuring the redundant-
  walk fix above.** `gen_yaml` mirrors `gen_toml`'s shape and a new
  `apply(Move N source(s)) [yaml]` loop (N=1,4,8) exercises the same
  single-source/multi-source spread. Before/after numbers (median of 10,
  `--nodes 300` and `--nodes 500` — `--nodes 5000`/`12500` as suggested were
  impractically slow to complete because of TOML's separately-known-and-
  already-tracked `Move` scaling, not this fix): at 300 sections, Move
  1/4/8 went 13.5ms/28.5ms/48.6ms → 11.6ms/26.7ms/46.5ms; at 500 sections,
  22.9ms/48.5ms/82.1ms → 19.7ms/45.8ms/79.1ms. The ~2-3ms improvement is
  roughly constant across source counts and grows with document size,
  matching the fix removing exactly one whole-document walk per `Move`
  call rather than one per source.

### Unreleased Update — 2026-08-30T00:19:34Z
- **perf(core/yaml): `Move` no longer recomputes the same tree projection it
  was already handed.** `move_nodes`'s pre-deletion shift calculation called
  `project(tree, "")` to look up the target's parent, redundantly repeating
  the exact same `walk()` the `apply` dispatcher had already run to build
  its `idx`/opaque-check. `apply` now keeps the `NodeTree` half of that walk
  (previously discarded as `let (_, idx) = walk(...)`) and threads it into
  `move_nodes` as a new `proj` parameter, which uses it directly instead of
  re-walking. The per-deletion re-walk inside the delete loop is unchanged —
  each deletion splices the tree, so that one is genuinely required.

### Unreleased Update — 2026-08-29T23:56:48Z
- **docs: document array-element `Remark` as YAML-only by design.**
  `Remark` on an array/sequence element is supported by YAML (comments are
  first-class per-item tokens) but returns `Unsupported`/`Illegal` on
  TOML/JSON, whose array syntax has no natural per-element comment slot.
  Recorded as an intentional format-capability difference, not a bug, in
  `docs/reference/CONTEXT.md`'s Mutation mechanics table.

### Unreleased Update — 2026-08-29T23:56:26Z
- **fix(core/json): `Rename` no longer corrupts a key containing a quote or
  backslash, and its collision check now compares decoded keys.** `rename`
  built its probe fragment by blindly interpolating the raw `new_key` between
  quotes (`format!("{{\"{new_key}\": 0}}")`), so a `new_key` that already
  carried a `"` produced malformed/double-quoted JSON; the sibling-collision
  check also compared the raw `new_key` against decoded sibling names instead
  of decoded-to-decoded. `rename` now escapes/wraps a bare `new_key` (or uses
  an already-quoted one as-is), parses the probe first, decodes the new key
  via the existing `key_name_of` helper, and only then runs the collision
  check against other decoded sibling keys — matching TOML/YAML's existing
  behavior.

### Unreleased Update — 2026-08-29T23:55:43Z
- **fix(core/json): `Insert` on an empty or comment-only `.json` document no
  longer fails with `NotFound`.** JSON's `find_container` had nothing to walk
  into on a document with no top-level VALUE node, so pressing "Add" on a
  brand-new/empty `.json` file always failed — TOML and YAML both already
  synthesized a root container in this case. Added `insert_into_empty_document`
  (mirrors YAML's `insert_into_empty_document`), wired into `insert()`'s
  `find_container` call the same way YAML does. Defaults to an object root
  (`{}`), matching TOML's root-is-always-Table convention.

### Unreleased Update — 2026-08-29T14:35:23Z
- **perf(core): drop the redundant full-document serialize on every
  mutation's undo snapshot.** `ConfigDocument::apply` already computes the
  post-mutation serialized text internally (for its own DOM
  validation/reparse) and threw it away; `Session::on_mutation_success` then
  called `doc.serialize()` again to build the undo-history snapshot — a
  second full-tree-to-string pass on every keystroke that commits a
  mutation. `apply` now returns that text (`Result<String, MutateError>`
  instead of `Result<(), MutateError>`), and `on_mutation_success` takes it
  as a parameter instead of recomputing it. No behavior change: `cargo test
  --workspace` and `cargo clippy --workspace -- -D warnings` both green.

### Unreleased Update — 2026-08-29T14:34:50Z
- **perf(core/toml): `Move`/`Insert` no longer re-walk the tree twice per
  fragment.** `move_nodes`'s per-fragment reinsertion loop, and `insert`'s own
  per-table-member loop, each computed a fresh projection via `walk(tree, "")`
  to locate the next insertion index and then called `insert`, which
  immediately re-walked the same unchanged tree before doing anything with
  it — doubling the walk count in both hot loops. `insert` is now split into
  a thin public wrapper and a private `insert_with` that takes the caller's
  already-computed projection/index instead of recomputing them; both loops
  call `insert_with` directly. Verified with the perf harness's `apply(Move
  N source(s))` cases: `apply(Move 1 source(s))` dropped from 87.5ms to
  71.8ms at 2,801 nodes and from 752ms to 579.8ms at 7,001 nodes.

## [v0.23.0] - 2026-08-29

### Unreleased Update — 2026-08-29T00:00:00Z
- **fix(tui/core/yaml): `$EDITOR` on a comment node keeps nested indentation;
  quit-without-save no longer mutates the document.** Follow-up to the
  un-remark indentation fix: the external-editor *initial text* for a comment
  node came from the DOM projection, whose comment merge drops each line's
  leading INDENT — so opening a nested remarked block from the (collapsed)
  comment row showed every line flattened, and exiting the editor handed that
  flattened buffer back, which was spliced in even though nothing was saved.
  Both the TUI `edit_node` comment branch and the core `external_edit_view`
  handshake now source the initial from the document's CST
  `serialize_fragment` (per-line indent preserved), and an unmodified buffer
  is treated as cancel (no splice, no dirty flag).

### Unreleased Update — 2026-08-29T00:00:00Z
- **fix(core/yaml): un-remark preserves nested indentation.** Remarking a
  nested block entry (e.g. a `subscribers:` subtree) and un-remarking it
  flattened every line to the comment block's first-line indent, turning
  `subscribers` into `null` and lifting `error:` one level up. Root cause:
  `comment_block_text` dropped each line's INDENT token when collecting a
  merged `#` block, and the reverse splice reindented the indent-less text
  uniformly. `comment_block_text` now keeps per-line leading indent, and the
  reverse-remark path dedents relative to the block's own first-line indent
  before the parse check and re-splice. Round-trip verified byte-exact against
  the reported fixture (`/tmp/verify-test/tasks.yaml`).

### Unreleased Update — 2026-08-29T08:11:21Z
- **perf(core): stop rescanning the whole document per section span; benchmark
  Move and locate the remaining bottleneck.** Adding a multi-source `Move`
  case to the perf harness exposed a gesture far slower than anything the
  audit had predicted: on a 400-section / 87 KB document a **single-section
  Move took 636ms**, and an 8-source Move 5.07s. Profiling (macOS `sample`
  against a release build with symbols) put 38% of it in one function.
  - `section_end_strict` / `section_end` / `aot_entry_end` each did
    `tree.children_with_tokens().collect()` — materializing a rowan cursor for
    **every** top-level element in the document — only to scan forward from one
    header. `table_member_spans` calls that once per member, so a
    single-section delete was O(document × members). Added
    `section_end_strict_from` / `section_end_from` / `aot_entry_end_from`,
    which walk `next_sibling_or_token` from the header node and so visit only
    the elements between it and the next header. Same scan, same predicate,
    same result — it just starts where the answer is. Migrated the 11 call
    sites that already hold the header node (delete, remark, replace-spans,
    section-span-text, AoT entry body); the index-only callers in
    `aot_group.rs` keep the original form. `Move 1` **636ms → 419ms**,
    `Move 8` **5.07s → 3.29s**.
  - **The remaining cost is architectural, and now measured.** `apply` walks
    the `clone_for_update` (mutable) tree, and rowan's mutable cursors are
    far more expensive than immutable ones: instrumenting `walk` showed
    **47.5ms per walk on the mutable tree vs 3.9ms on the immutable one — 12x**
    — so the 4 walks a single Move performs account for ~190ms of it, and
    `NodeData::new` is 93% of the whole mutation. The walks cannot simply be
    hoisted: the `CstIndex` they build holds `SyntaxNode`s that must point into
    the tree being spliced. Making Move fast needs a design change (resolve
    paths to child indices on the immutable tree, then navigate the mutable
    tree positionally), not another local fix — recorded here rather than
    attempted, since it touches the most correctness-critical code in the
    crate. Per-keystroke paths are unaffected: `Replace`/`Rename` are ~11ms
    and `visible_rows()` ~1ms at this size.
  - The harness gains `apply(Move N source(s))` for N = 1/4/8 so the above is
    falsifiable and regressions are visible.

### Unreleased Update — 2026-08-29T07:37:33Z
- **perf(core): make `project()` linear and drop one of the two
  serialize+reparse cycles every mutation ran.** Follow-up to the
  `06:42:44Z` audit entry, which left `apply` as the dominant cost and
  flagged `project()` as unexpectedly superlinear. A scaling sweep confirmed
  it: `project()` grew **3.1–3.5× per doubling** of document size (≈O(n^1.7)),
  not 2×. Two independent causes, both now fixed. Measured with
  `cargo bench -p confy-core --bench perf -- --nodes N`:

  | | 200 sec | 400 sec | 800 sec | 1600 sec |
  |---|---|---|---|---|
  | `project()` before | 2.99ms | 8.38ms | 29.07ms | 90.62ms |
  | `project()` after | 2.55ms | 4.97ms | 9.10ms | **19.97ms** |
  | `apply(Replace)` before | 7.28ms | 18.05ms | 49.68ms | 143.63ms |
  | `apply(Replace)` after | 5.65ms | 11.37ms | 25.43ms | **59.34ms** |

  Per-doubling growth for `project()` is now 1.95 / 1.83 / 2.19 — linear.
  - **The quadratic term was `cst_project::node_at_mut`'s descent.**
    `append_child` calls it once per projected entry to find the enclosing
    scope, and it scanned `root.children` — one child per section — linearly,
    so the cost was O(sections² × entries) `Vec<Seg>` comparisons, each one
    comparing `String` key segments. Two fixes, both licensed by the existing
    "every child's path is its parent's path plus one segment" projection
    invariant (already documented on `NodeTree::node_at` and relied on by
    `visible_rows`): compare only the **last** segment rather than re-walking
    the whole `path[..=i]` prefix the parent already matched, and scan children
    from the **back**, since the walk fills the tree in source order and the
    target scope is almost always the most recently appended child. Sibling
    paths are unique, so scan direction cannot change which node is found.
  - **Every mutation serialized and re-parsed the whole document twice.**
    `validate_semantics` needed a serialize + re-parse for its duplicate-key
    check, and the document's `apply` then needed a serialize + re-parse to
    normalize the `clone_for_update` tree back to an immutable one — the same
    work, back to back. The validation re-parse *already produces exactly the
    normalized tree the caller wants*, so it is no longer thrown away:
    `apply` now returns `(SyntaxNode, String)` and the caller commits both.
    Applied to all three backends (`cst_edit`, `json::edit`, `yaml::edit`).
    For TOML the DOM check and the normalized tree now share one parse via a
    `Parse` clone (only a green-node refcount bump).
  - The doc-level re-parse used to map failures to `MutateError::Fragment`,
    but `validate_semantics` had already parsed the identical text and
    returned `Illegal` on failure, so that arm was unreachable — no behavior
    change. `yaml::edit::apply_str` and the JSON `apply_str` test helper get
    simpler, returning the serialization `apply` now hands back instead of
    re-serializing.

### Unreleased Update — 2026-08-29T06:42:44Z
- **perf: cut per-keystroke core cost ~14x and native runtime ~2.2x; add a
  perf harness.** A read-only optimization audit found the interactive hot
  path doing whole-document work per keystroke, and the size-optimized
  release profile applying to native binaries. Measured on a 43 KB /
  2801-node TOML document (`cargo bench -p confy-core`):
  - `Session::visible_rows()` **6.32ms → 521µs**. `path_display` was built
    by `human_path`, which calls `NodeTree::node_at` once per path segment
    — a linear child scan each time, so O(depth² · siblings) *per row*, ~92%
    of the function's cost. It is now built incrementally down the ancestor
    chain (each row's display path is its parent's plus its own segment),
    which is sound because `flatten` is pre-order so ancestors always precede
    a row. The chain is built over the unfiltered flatten and the filter
    applied after, so an active filter can't punch holes in it.
    `to_view_row` now takes `path_display` as a parameter; `view_row_at`
    (single-row, no chain) still uses `human_path`.
  - `ConfigDocument::is_dirty()` **900µs → 0ns**. It re-serialized the entire
    document and string-compared it on every call, and the `clean` fast-path
    flag stopped covering the case that matters — it is cleared on the first
    edit and only restored on save, so every keystroke of an editing session
    paid a full serialization (it is read per-dispatch via `SessionSnapshot`).
    All three backends now recompute a `dirty` flag at the four points that
    change the text; `apply`/`replace_from_str` already had the new
    serialization in hand, so the comparison is free. Edit-then-undo still
    reads clean (compared against the baseline text, not a sticky flag).
  - Native `[profile.release]` **`opt-level = 'z'` → `3`**, worth ~2.2x
    runtime: `apply(Replace)` 16.67ms → 7.59ms, `project()` 8.08ms → 3.31ms,
    `serialize()` 948µs → 482µs. Size-optimizing the TUI and desktop binaries
    bought nothing users feel. The wasm bundle is the one artifact where size
    wins, so `web/cf-build.sh` overrides just that leg with
    `CARGO_PROFILE_RELEASE_OPT_LEVEL=z` (the same env-var idiom
    `release.yml` already uses for the Windows builds). Verified: without the
    override the wasm grows 2795 KB → 3876 KB.
  - `web/build.mjs` now sets esbuild `minify: true`. esbuild does **not**
    minify by default even in bundle mode, so `ui.js` was shipping as ~5200
    lines of indented, commented source: 212 KB → 144 KB (brotli 41 → 33 KB),
    `touch/app.js` 201 KB → 142 KB (brotli 37 → 31 KB).
  - New `crates/confy-core/benches/perf.rs` (`[[bench]]`, `harness = false`,
    **no new dependency** — a plain `main()` with median-of-N timing rather
    than criterion, to keep the dependency graph tight). Covers parse,
    project, serialize, `is_dirty`, `apply`, and `visible_rows`; takes
    `-- --nodes N`. There was previously no benchmark or large-document
    fixture anywhere in the workspace, so no optimization claim was checkable.
- **fix(core): `delete_selected` snaps the cursor to the deletion point.**
  It computed the topmost deleted row index into `first_idx` and then never
  used it (the sole `cargo clippy` warning in the workspace, so
  `clippy -D warnings` was failing its documented pre-commit gate), leaving
  `compute_rows`'s unresolvable-cursor fallback to dump the cursor on row 0 —
  deleting deep in a large file sent the user to the top. The cursor now lands
  on the row that took the deleted rows' place, clamping to the last row when
  the tail was deleted. **Supersedes the vanish-on-delete contract** recorded
  in the `2026-08-29T00:21:42Z` entry: `cursor_row()` is now live immediately
  after `delete_selected` instead of returning `None` until the host's
  `compute_rows()`. Hosts calling `compute_rows()` are unaffected (it leaves
  an already-valid cursor alone). `cursor_row_tracks_cursor_across_a_mutation`
  keeps asserting the invariant it was written for — a dead path must not
  yield a stale row — via `view_row_at(&deleted_path).is_none()`.

### Unreleased Update — 2026-08-29T04:19:44Z
- **docs(core): consolidate multi-selection semantics into
  `ROW_STATE_MODEL.md` §1c as the SSOT.** New section records the
  `selected_paths()` contract (selection outranks cursor;
  `normalize()` drops descendants), the selection-aware op table
  (delete dead-path drop / copy-cut freeze / remark post-image remap /
  rename prefix remap / paste clear) and remark's three post-image
  shapes with the top-down processing invariant and regression-test
  pointers. Fixes drift from `44a0f8b`: ADR 0005 gains a
  later-revision note superseding its single-focal-row framing of
  remark; `ROW_STATE_MODEL.md` §1 state #2 and `CONTEXT.md`'s
  Remark/Locked-selection glossary entries now describe the
  selection-aware behavior.

### Unreleased Update — 2026-08-29T00:21:42Z
### Unreleased Update — 2026-08-29T04:19:44Z
- **fix(core): multi-select now follows remark collapse/expansion;
  delete no longer leaves a dead selection.** Remarking adjacent selected
  rows merges them into one comment block, and un-remarking a selected block
  splits it back into several rows — but the selection kept the stale
  pre-mutation paths, so every later operation (remark/copy/paste) silently
  hit NotFound until Esc. `Session::remark` now remaps the selection onto
  each remark's post-image, processed top-down so merges only ever fold
  upward: in-place kind swaps (Key↔Index) track the swapped address, an
  adjacent-row merge remaps onto the merged block (select a,b → remark →
  block selected → remark → both restored), and un-remarking a selected
  block expands the selection onto all restored rows. `delete_selected`
  drops selected paths that no longer resolve (co-selected live paths are
  kept). The cursor keeps its existing vanish-on-delete contract
  (`cursor_row()` returns None until the host's `compute_rows()` snap).

### Unreleased Update — 2026-08-29T01:25:49Z
- **fix(core): remark now acts on the active multi-select.** `Session::remark`
  only ever targeted the cursor row, unlike `delete_selected`/copy/cut which
  prefer an active selection (`selected_paths()` falling back to the cursor).
  Remark now follows the same contract: with `s`/Shift-range selection active,
  all selected nodes toggle Node<->Comment (applied deepest-first so a
  container's re-addressing — key<->positional — cannot orphan a still-pending
  descendant; per-node Fragment errors like prose comment blocks surface as
  the usual "kept as-is" notice while the rest still apply). TOML and
  JSON/JSONC covered by headless regression tests; TUI/web dispatch untouched
  (both forward `Intent::Remark`).

- **fix(core): un-remark of a merged multi-node comment block works in
  JSON/JSONC and YAML.** Remarking several consecutive nodes merges their
  comment lines into ONE Comment node; un-remarking that node failed with
  "not valid comment, kept as-is" for JSON/JSONC and YAML (TOML was fine —
  its reverse remark strips and reparses the whole block with no
  single-entry requirement). Both reverse paths now mirror TOML:
  - JSON/JSONC: the recovered block is split into member fragments
    greedily — extend the candidate until it parses as a single member
    (`parse_member_fragment` already enforces exactly one), which is exact
    because no proper prefix of a JSON member ever parses. Every fragment
    is re-inserted as its own item, each keeping its trailing `//` comment
    via `TRAILING_MARKER`; multi-line members reassemble from their
    multiple `//` lines.
  - YAML: the reverse validation required the recovered block to parse as
    exactly ONE map entry (`parse_map_entry_fragment`'s count check); it
    now accepts any block that reparses with at least one MAP_ENTRY or
    SEQ_ENTRY at top level (`parses_as_live_entries`), and the existing
    byte-splice restores all of them at the block's original indent.
  - YAML forward rounding also fixed: remarking further entries in a
    container that already holds a comment re-emitted that older comment
    at column 0 (`collect_items` dropped the comment line's INDENT token
    while entry items keep theirs), corrupting nested-block indentation;
    and after remarking a nested table's LAST entry the remaining comments
    were swallowed invisibly into the valueless entry — the parser now
    wraps deeper comment-only trivia in a comment-only child MAPPING so
    the rows stay visible and re-addressable (same-indent trivia still
    floats at the parent level).
  Regression tests cover root pair, nested pair, sequence pair (YAML) and
  single-line plus multi-line merged blocks (JSONC), forward and back.

### Unreleased Update — 2026-08-28T23:45:38Z
- **fix(core): JSON/JSONC remark keeps trailing comments; YAML block leading
  comments project.** Two comment-loss bugs, both fixed with regression tests.
  - JSON/JSONC `Remark`: commenting out a member with a trailing `//` comment
    (`"a": 1, // t`) dropped the trailing comment entirely (the item is
    rebuilt from the bare member text); the reverse direction re-inserted the
    recovered member so the comma landed *after* the comment (parse error).
    Forward now appends the trailing comment to the commented block's last
    line (`// "a": 1  // t`); reverse splits it off via the CST and re-merges
    it with `TRAILING_MARKER` so `rebuild_*` keeps it last, after the comma.
  - YAML: a nested block mapping/sequence's *leading* comment lines
    (`srv:\n  # c\n  host: a`) were skipped into the parent
    MAP_ENTRY/SEQ_ENTRY by `parse_value`/`parse_seq_entry` before the child
    block node started, where floating trivia is invisible to projection —
    the table's first tree row could never be a comment. The child-block
    decision now uses a non-consuming lookahead (`peek_line_after_trivia`)
    that skips blank/comment-only lines; when a child block follows, its own
    loop-start `skip_trivia_lines` floats the trivia inside the child, so
    `walk_mapping`/`walk_sequence` project it as a proper Comment node.
    Implicit-null entries keep the old trivia placement.

### Unreleased Update — 2026-08-28T23:10:00Z
- **test(core): validator/resolver parity regression test.** The 2026-08-24
  (`patternProperties`) and 2026-08-29 (`additionalProperties`) schema-info
  fixes were the same bug twice — `hints_edit.rs::resolve_subschema`'s
  keyword whitelist lagging behind `validate.rs`'s full jsonschema validator.
  New `#[cfg(test)]` parity tests in `hints_edit.rs` lock the invariant
  directly: a schema exercising every applicability keyword
  (`properties`/`patternProperties`/`additionalProperties`/`items`/`$ref`)
  is compiled with the real validator, and every path it flags must resolve
  through the hint walker. Future keyword-whitelist gaps now fail a test
  instead of silently dropping detail-panel schema info.

### Unreleased Update — 2026-08-28T22:47:27Z
- **fix(core): schema info resolves through `additionalProperties`.** The
  Detail panel's `Schema:` info section (description/`Type:` line) silently
  vanished for any node defined under a dictionary-style schema's
  `additionalProperties` — the compiled-validator path (`validate.rs`) always
  enforced that keyword, but the best-effort hint/info resolver
  (`hints_edit.rs::resolve_subschema`) only walked
  `properties`/`patternProperties`/`items`/`$ref`, so task-level entries
  resolved to "no info" while violations still rendered. Added
  `additionalProperties` as a `Seg::Key` fallback (a JSON-schema bool there
  harmlessly yields no info). Surfaced most visibly in the TUI `i` Detail
  popup; verified live on a dictionary-of-tasks fixture (`put_in_key` now
  shows `Schema: / Type: object`). Two new `schema_headless` tests cover the
  schema-object and bool forms.

### Unreleased Update — 2026-08-28T22:11:36Z
- **chore: audit remediation — Rust hygiene, panic hardening, CI gates.** Fixed
  every quick-win/medium finding from the 2026-08-29 codebase audit (version
  drift out of scope, tracked separately by the maintainer).
  - `cargo fmt --all` across ~25 previously-unformatted files; fixed the 4
    mechanical `cargo clippy` warnings (`redundant_closure` in `convert.rs`,
    `doc_lazy_continuation` in `view.rs`, `useless_asref` ×2 in `keys.rs`,
    `manual_flatten` in `tests/prompt_question.rs`); annotated the 6
    `too_many_arguments` sites (`yaml/project.rs`, `session.rs`,
    `type_filter.rs`) with `#[allow(...)]` + a justification comment.
    `cargo clippy --workspace --all-targets -- -D warnings` is now 0
    warnings.
  - `crates/confy-tui/src/tui/schema_io.rs`: added a 10s `.timeout()` to the
    blocking schema-URL fetch so a slow/unresponsive host can no longer
    freeze the TUI indefinitely.
  - `crates/confy-core/src/session/clipboard.rs`: `move_selection_to`'s
    `self.doc.unwrap()` converted to a local `let-else` guard, matching the
    pattern already used by every other function in the file.
  - `web/confy.ts`'s `Session.dispatch()` and
    `editors/vscode/src/schemaSessionManager.ts`'s `reparse()`/`syncSchema()`
    wasm dispatch calls now catch and log core-side panics instead of
    surfacing an unlabeled wasm trap.
  - `crates/tauri-plugin-confy-picker/Cargo.toml` now inherits
    `authors`/`license` from the workspace and has a real `description`.
  - Added `.github/workflows/rust-ci.yml` (fmt --check, clippy -D warnings,
    cargo test) and `.github/workflows/vscode-ci.yml` (typecheck + the new
    `npm test` script) — both visibility-only, mirroring `web-ci.yml`'s
    single-committer-repo rationale. Wired `editors/vscode`'s existing
    `src/*.test.ts` suite into a `package.json` `test` script (28/28
    passing); it previously ran only via manual `node
    --experimental-strip-types --test`.

### Unreleased Update — 2026-08-28T21:00:00Z
- **docs: repo-wide documentation accuracy + organization pass.** Audited every
  current-state doc against the code and corrected what had drifted; historical
  records (plans/specs/audits/ADRs and this changelog) were left intact, gaining
  only status banners.

  Corrected factual errors:
  - `CLAUDE.md` still listed `supports_comments()` as a live `ConfigDocument`
    format facet and claimed `load_document` "enables JSONC comments for a
    `.jsonc` extension" — both removed with the comment write-gate
    (`7b5cfda`/`23e6731`). Now documents `had_comments_at_open()` and the
    extension-blind load path. The trait's method list also dropped a
    nonexistent `load` and gained `to_value()`.
  - `PRIVACY.md` (+ its `web/privacy.html` mirror) claimed the "only network
    request" was the optional Open-from-URL feature. It missed the **JSON Schema
    fetch** (`tui/schema_io.rs`, `web/ui.ts`), which fires automatically when an
    opened document declares a schema by URL. Both egress paths are now
    disclosed, and "runs entirely offline" is restated accurately. Store
    listings point at this text, so the omission was material.
  - `MESSAGES.md` §1 said "two channels" while its own table listed three, and
    asserted every channel holds one value — untrue of the 256-entry `DiagRing`.
  - `RELEASES.md` still advertised v0.22.0 as current on four channels; the
    released tag is v0.22.1.
  - `docs/superpowers/plans/2026-08-28-key-repr-first-class-literal.md` was still
    marked "proposed, NOT started" after shipping.

  Documented what the last three commits added but never wrote down: the
  decoded-vs-authored key contract (`Node.key_literal`, `rename_key_segs`), the
  Path line (`Session::human_path`, `ViewRow.path_display`) — new CLAUDE.md
  *Key representation* section and two CONTEXT.md glossary entries, plus TUI.md's
  `Path:` line. `WEBUI.md`'s FFI table gained the 8 missing bindings, its
  `ViewRow`/`SessionSnapshot` field lists were completed, and a new note records
  that wasm-bindgen exports **snake_case** while `web/confy.ts` wraps it in
  **camelCase** — the VS Code extension binds the raw snake_case names, so both
  spellings must move together.

  Organization: `yaml-quoted-key-edit-memo.md` (self-declared RESOLVED, describing
  a `key_literal_text` mechanism that no longer exists) moved out of the
  current-state `docs/reference/` into `docs/superpowers/debug/`; the false-start
  record is preserved, its inbound links repointed. Added
  `docs/superpowers/README.md` and `docs/adr/README.md` indexes (previously none),
  and the standard status banner to the 10 finished docs that lacked one — so an
  unbannered file now reliably means live work (currently just the JSON/JSONC
  parser-simplification SSOT plan).

### Unreleased Update - 2026-08-28T18:00:00Z
- **fix(core): renaming a key to add quotes no longer raises a bogus type-change
  prompt and a following "path not found"** (reported on both TUI and Web).
  Follow-up to the `key_literal` refactor below, which taught the rename buffer
  to carry a key's authored spelling but left the **inverse** direction unfixed:
  after `Mutation::Rename` the session set the path's leaf segment to the raw
  literal (`Seg::Key("\"a\"")`) while projection builds paths from **decoded**
  keys (`Seg::Key("a")`). Every later `node_at` on that path missed, so
  - the type-change check read the node's kind as `Root` instead of its real
    type, and any key/value commit (the detail panel's, which is not
    `rename_only`) tripped a `PromptKind::TypeChange` confirmation, and
  - confirming it ran `apply_replace` on the stale path -> `path not found`.

  The rename itself had already succeeded, which is why the file still ended up
  correct - the failure was pure path bookkeeping. New `ConfigDocument::
  rename_key_segs()` decodes a rename's literal with the backend's own key lexer
  (TOML reuses `cst_project::key_segments` via `decode_key_source`; YAML reuses
  the same `parse_map_entry_fragment` + `entry_key_name` pair its `rename`
  mutation already uses for collision checks; JSON takes the default, its keys
  being decoded by contract). The two hand-rolled path remaps collapsed into one
  `remap_renamed_path` helper.

  This also removes a pre-existing `new_name.split('.')` in
  `apply_deferred_rename` that shattered a quoted key containing a dot
  (`"a.b"` became two segments and wrote a mangled leaf).
- **fix(web): the detail panel's Key field shows a key's authored spelling.**
  `panel.ts` still seeded the editable Key input from the decoded `row.key`, so
  a quoted key's quotes were invisible there and reopening the panel appeared to
  have lost them. Worse, the field is committed verbatim: an otherwise untouched
  panel commit would silently restyle a quoted key to bare. Now reads
  `key_literal ?? key`, matching the tree row and the rename input. Covers the
  shared touch + desktop panel; the TUI Detail popup has no separate Key field
  (its "Path:" line already uses `path_display`).
- test: `crates/confy-core/tests/key_repr.rs` gained four cases (F2 and
  detail-panel quote-adding renames across TOML/YAML, the unquote and requote
  directions, and `rename_key_segs` decoding without splitting a quoted dot);
  `web/render.spec.mjs` gained three `panelHTML()` Key-field cases.

### Unreleased Update — 2026-08-28T14:30:00Z
- **fix(core,web,tui): made a key's authored spelling a first-class projection
  output (`Node.key_literal`), replacing the lossy `key_sign`-plus-synthesized-
  quotes approach that four prior commits (`64db70a`, `af6adc7`, `4795e89`,
  `8ef6af0`) kept patching site by site.** Plan:
  `docs/superpowers/plans/2026-08-28-key-repr-first-class-literal.md`.

  The three backends now agree on one contract: `Node.key`/`Seg::Key` hold the
  **decoded** key (semantic identity — path resolution, collision checks,
  JSON-Schema `properties` lookup, `to_value`/convert), while the new
  `Node.key_literal: Option<String>` holds the key **exactly as authored**
  (presentation + edit identity — tree row, Path line, rename/edit buffer).
  Filled once during projection from the key token already in hand, so no
  consumer re-walks the CST or invents a quote character.

  Fixed, each verified on the real binary:
  - **A single-quoted YAML key rendered with double quotes** everywhere the key
    was displayed — Detail "Path:" line, `ViewRow.path_display`, and the tree
    row. Three separate call sites hardcoded `'"'`
    (`session.rs::human_path`, `confy-tui/src/tui/ui.rs::display_key`,
    `web/kind-labels.ts::displayKey`) because `KeySign::Quoted` records *that*
    a key was quoted, never *how*. `'a b'` now shows `'a b'`.
  - **TOML→JSON conversion corrupted a quoted key**: `"a b" = 1` converted to
    `{ "\"a b\"": 1 }`. taplo lexes a quoted key as an `IDENT` whose text keeps
    the quotes, and `cst_project::key_segments` read it raw, so `Seg::Key` was
    `"\"a b\""` (quotes included). Now decoded; converts to `{ "a b": 1 }`.
    The same leak silently defeated JSON-Schema `properties` lookup
    (`schema/hints_edit.rs`, `schema/dirty_check.rs`) for every quoted TOML key.
  - **YAML's double-quoted decoder ignored `\xNN`/`\uNNNN`/`\UNNNNNNNN`**,
    leaving the backslash sequence in the decoded key. Because YAML considers
    `"a\x20b"` and `a b` the same key, the rename collision check could not see
    the clash and silently wrote a document with two identical keys. Renaming
    into that clash is now correctly rejected.
  - `ConfigDocument::key_literal_text()` **deleted** — a YAML-only side channel
    that re-`walk()`ed the whole tree per call (O(n)) and maintained a second,
    drift-prone KEY-token lookup. Its two callers read `ViewRow.key_literal`.
  - Per-format special cases **deleted**: `is_quoted_yaml_key` /
    `isQuotedYamlKey`, the `DocFormat::Toml` "don't double-wrap" branch, and the
    `DocFormat::Json` "don't wrap" branch. `web/kind-labels.ts::displayKey` was
    inlined at its two call sites (project rule: no one-expression wrappers).
  - `Seg::Key` for TOML is now decoded, so `cst_edit/tree_nav.rs::fragment_key_segs`
    decodes identically (it had the same raw-`IDENT` bug) to keep collision
    detection consistent. `quote_key_seg`/`path_key_display`/`prefix_section_headers`
    already re-quoted decoded segments, confirming decoded was the original intent.

  Scope notes: `Node.key_sign` is **kept** as a stored coarse facet — it also
  carries `Dotted`, which `key_literal` cannot express — but is now documented as
  never usable to reconstruct a spelling. JSON's `key_literal` is `None` by
  contract: its keys are unconditionally `"…"`-quoted, so the authored spelling
  adds no information, would put redundant quotes on every row, and would feed
  `"key"` into a rename that re-quotes the name it is given.

  `ViewRow` gained `key_literal` (additive; no `serde(deny_unknown_fields)`), and
  the TUI's `RowSnapshot` mirrors it. Tests: `cargo test --workspace` green (31
  binaries); the golden TOML projection expectation was updated (it pinned the
  quote-leak bug); new coverage for single-quoted Path/tree-row rendering in
  `session_headless.rs`, `confy-tui/src/tui/ui.rs` and `web/render.spec.mjs`, plus
  `\x`/`\u`/`\U` decoder tests.

### Unreleased Update — 2026-08-28T09:00:00Z
- fix(core,web,tui): completed the quoted-YAML-key rename/Path-display fix
  (see `docs/superpowers/debug/2026-08-28-yaml-quoted-key-edit-memo.md`). Reverted the earlier
  decoration-only patch (`af6adc7`) now that the rename/edit buffer itself
  carries the literal quote characters (`key_literal_text`, prior commit) —
  the quote marks are ordinary, directly editable buffer content, mirroring
  TOML exactly (editable quotes, an inside-quote trailing space survives
  `edit_commit`'s `.trim()`). `web/render.ts`'s `.key-quote` decoration span
  and CSS rule and `crates/confy-tui/src/tui/ui.rs`'s matching span pair are
  removed. New `Session::human_path()` (`crates/confy-core/src/session/session.rs`)
  wraps a quoted-YAML-key path segment in display `"…"` flanks; used by
  TUI's Detail popup "Path:" line and a new `ViewRow.path_display` field
  consumed by `web/panel.ts`'s Path field — TOML/JSON unaffected (gated on
  YAML + `KeySign::Quoted`). Also fixes a value-only edit silently dropping
  a quoted key's quotes (found as a side effect of the prior commit). Ten
  new `crates/confy-core/tests/session_headless.rs` tests cover Path-line/
  `path_display` quoting, the value-only-edit fix, and scripted end-to-end
  scenarios (quote-char editing + inside-quote trailing space, no-op
  commit, collision typed with/without quotes). Rewrote the two
  decoration-focused regression tests to assert the literal-quoted buffer
  value instead.

### Unreleased Update — 2026-08-28T07:31:25Z
- fix(web,tui): follow-up to the quoted-YAML-key tree display fix above — the
  quote marks it added were still momentarily invisible the instant `F2`/rename
  started, because the Name-edit `<input>`/buffer only ever held the decoded,
  unquoted key text (unlike TOML, whose key string carries its quotes inline,
  so its rename input never visually changes). Added a display-only `"…"`
  flank around the editable field for a quoted YAML key — static decoration,
  not part of the input's value, so rename commit/collision logic is
  untouched. `web/render.ts::renderRow` (new `.key-quote` span + CSS),
  `crates/confy-tui/src/tui/ui.rs`'s Name-field render branch. New shared
  `isQuotedYamlKey` helper (`web/kind-labels.ts`) backing both `displayKey`
  and the new decoration. New `web/render.spec.mjs` cases,
  `crates/confy-tui/src/tui/ui.rs::quoted_yaml_key_rename_shows_quote_flanks_around_edit_buffer`.

### Unreleased Update — 2026-08-28T07:16:41Z
- feat(web): Save As/Convert gains a **JSONC** option alongside JSON, TOML, and YAML —
  the same `DocFormat::Json` target with `.jsonc` seeded instead of `.json` (core stays
  extension-blind by design; see the 2026-08-28 comment-gate-removal work). The picked
  pseudo-format is derived from the current output path's extension on every re-render
  (`uiTagFor` in `web/convert-dialog.ts`), not separate host state, so it survives
  snapshot updates without being clobbered back to plain JSON. New `web/convert-dialog.spec.mjs`.
- feat(tui): the Convert flow's Path step gains a `Tab` key that toggles the output
  path's extension between `.json` and `.jsonc` when the target is JSON
  (`App::convert_toggle_jsonc_ext`) — the Format-step picker itself stays 3 options
  (TOML/JSON/YAML) since its cursor bounds are core-driven and `DocFormat` has no
  `Jsonc` variant. Documented in the `?` help text (`tui.help.*` catalog, en + zh-TW).
- fix(web,tui): a **quoted YAML key** (`"a b": 1`) never showed its quote marks in the
  tree row — only in the raw-text view — unlike TOML, whose quoted keys already show
  quotes (an existing quirk of how `taplo` lexes them, not a deliberate feature).
  `key_sign`/`Sign` was already tracked but only surfaced in the detail popup, never
  the row label. Added a display-only `"…"` wrap for `key_sign === Quoted` YAML rows
  in the tree (`web/kind-labels.ts::displayKey`, `crates/confy-tui/src/tui/ui.rs::display_key`)
  — informational only, never fed back into rename/edit/collision logic, and skipped for
  TOML (already quoted) and JSON (unconditionally quoted, would be pure noise). New
  `web/render.spec.mjs` cases, `crates/confy-tui/src/tui/ui.rs` tests
  `display_key_wraps_quoted_yaml_keys_but_not_toml_or_bare_yaml` /
  `yaml_quoted_key_shows_quotes_in_tree_row`.
- feat(web): added an `F2` keyboard shortcut for renaming a key's name, mirroring the
  TUI's existing `KeyCode::F(2)` binding — previously the web tree (desktop and touch,
  external-keyboard) had no keyboard path into `BeginRename`, only a mouse click on the
  key label. One change in the shared `resolveKeyIntent` (`web/key-intent.ts`) covers
  both desktop and touch. Documented in the Help overlay. New `web/key-intent.spec.mjs` case.

### Unreleased Update — 2026-08-28T06:14:27Z
- fix(tui): the previous issue-#4 fix (external/pop-up editor comment-clear revert) only
  patched the `Intent::ApplyReplace` dispatch handler used by web/vscode/tauri — `confy-tui`'s
  `edit_node()` `$EDITOR` commit calls `App::apply_replace` → `Session::apply_replace()`
  directly (a plain Rust method call, bypassing the `Intent` enum entirely), so TUI's external
  editor still silently restored a deleted trailing comment on JSON/TOML. Extracted the
  explicit-clear detection into a new shared `Session::apply_external_replace(path, text)`
  (wraps `apply_replace`, used only for the *external*-editor's authoritative full-fragment
  text — the pre-existing `apply_replace` keeps its "preserve unless `pending_trailing` set"
  semantics for the *inline* editor's value-only commits); both `dispatch.rs`'s
  `Intent::ApplyReplace` handler and TUI's `App::apply_replace` now call it. New TUI tests
  `external_edit_apply_replace_can_clear_{toml,json}_trailing_comment` (confirmed failing
  against the pre-fix `apply_replace` call, passing against `apply_external_replace`).

### Unreleased Update — 2026-08-28T05:20:52Z
- fix(web): `.json`-format **sample** documents (opened via New/loadSample, no real
  filename — `openSample` passes the literal name `"sample"`) never set `strict_json`,
  so `comment_advisory` never lit up when authoring a comment into one — the filename-regex
  check (`/\.json$/i.test(name)`) never matched. `web/ui.ts` and `web/touch/app.ts`'s
  `openText()` now derive `isPlainJson` from `format === "json" && (asSample || …)`. Real
  `.json` files were never affected (confirmed unrelated to the gate-removal work). Also
  fixed `web/touch/app.ts::openText()` missing the `strict_json`/one-shot
  `json-comments-detected` toast wiring entirely (present in `web/ui.ts`, absent on touch).
  New `web/sample-strict-json.spec.mjs`.
- fix(core): `Session::apply()`'s `ApplyOutcome` unconditionally drained `pending_schema_fetch`
  via `.take()` on *every* dispatched intent, and `dispatch()` overwrote the snapshot's
  `schema_fetch_request` with that drained value. A host that issues more than one dispatch
  right after opening a document (e.g. `web/ui.ts`'s `openText()`: `SetLang`, then
  conditionally `SetHostNotice` for the comment-advisory toast) before ever reading a
  snapshot's `schema_fetch_request` silently lost the schema fetch the very first of those
  dispatches drained it on — schema validation only "came back to life" once the user's first
  edit re-triggered detection via `sync_schema_hint()`. `pending_schema_fetch` now behaves like
  the already-correct `pending_external_edit`: it persists across unrelated dispatches
  (`.clone()`, not `.take()`) and is cleared only once `apply_schema_text()` actually resolves
  it. Added a `schemaFetchInFlight` guard in `web/ui.ts`/`web/touch/app.ts` so a still-pending
  request surviving across dispatches can't trigger a duplicate concurrent fetch. New
  `tests/session_schema_fetch_request.rs`.
- fix(core): JSON `insert` (`Intent::AddSibling`/`AddChild`) anchored on a node with a
  same-line trailing comment (`"a": 1  // c`) landed the new sibling *between* the value and
  its comment, detaching the comment into an independent standalone comment node —
  `model/json/edit.rs::collect_items_with_anchors()` gave a trailing comment its own
  CST-level item/slot, one more than the row projection ever exposed for that node (which
  folds a trailing comment into its owning row). A trailing comment is now merged into its
  owning member/element's item (an internal `TRAILING_MARKER` sentinel keeps the comma
  placement correct on rebuild), matching how TOML/YAML never gave a trailing comment its own
  slot in the first place — TOML and YAML were never affected. New
  `tests/insert_after_trailing_comment.rs`.
- fix(core): the external/pop-up editor's `Intent::ApplyReplace` never set `pending_trailing`,
  so clearing a node's trailing comment in the editor's bundled "value  # comment" text and
  saving silently brought the old comment back on JSON and TOML (not YAML) — `Mutation::Replace`
  defaults to preserving an existing trailing comment when the new fragment doesn't write one
  (`ConfigDocument::replace_preserves_trailing_comment() == true`, correct for the *inline*
  editor's value-only fragments, wrong for the external editor's authoritative full-fragment
  text). New `ConfigDocument::fragment_trailing_comment(path, fragment)` (implemented for JSON
  and TOML, delegating to the same fragment-parsing `Mutation::Replace` already uses
  internally) lets `ApplyReplace` detect an explicit clear and force it via
  `pending_trailing = Some(None)`. New `tests/external_edit_clears_trailing_comment.rs`.

## [v0.22.1] - 2026-08-27
### Unreleased Update — 2026-08-28T03:32:21Z
- feat(core)!: removed the JSON/JSONC comment write-permission gate. Authoring a comment into
  any `.json` document — via `remark` (turning a value into a comment) or `AddSibling`/
  `AddChild` next to an existing comment node — is now unconditionally legal, matching TOML/YAML,
  instead of being blocked until the file already had a `//`/`/* */` comment, a `.jsonc`
  extension, or the user accepted an interactive `JsoncUpgrade` `y`/`n` prompt.
  - `ConfigDocument::supports_comments()` and `JsonDocument.comments_enabled` are gone. In their
    place, `ConfigDocument::had_comments_at_open()` (default `false`; overridden by
    `JsonDocument`) reports a fixed, content-derived, non-writable fact — whether the file
    already contained a comment when it was loaded — used solely to drive the existing one-shot
    "this file already had comments" load-time toast (`tui.host.json-comments-detected` /
    `web.host.json-comments-detected`), decoupled from write permission.
  - Removed `PromptKind::JsoncUpgrade`, `PendingComment`, `PromptView::JsoncUpgrade`, the
    `Mode::Prompt(JsoncUpgrade)` accept-branch in `session.rs`, and the gate checks in
    `clipboard.rs::remark()`, `inline_edit.rs::add_comment_sibling()`, and
    `inline_edit.rs`'s `split_value_comment` call. Removed `AnyDocument::enable_comments()`,
    `confy-tui`'s `.jsonc`-extension `enable_comments()` call in `load_document`, and the
    `confy-ffi`/`web` `supports_comments`/`supportsComments` bindings (replaced by
    `had_comments_at_open`/`hadCommentsAtOpen`). Removed the now-dead i18n keys
    (`core.comment.unsupported`, `core.prompt.jsonc-upgrade`, `tui.prompt.jsonc-upgrade*`,
    `web.prompt.title.jsoncUpgrade`, `web.prompt.q.jsoncUpgrade`, `web.prompt.btn.upgradeJsonc`)
    and the `PromptView`/button-map entries in `web/types.ts`/`web/prompt.ts`.
  - Updated `CLAUDE.md` and `docs/reference/CONTEXT.md`'s "JSONC upgrade" glossary entry to
    describe the new always-legal behavior.
- test(core): added `remark_never_prompts_on_clean_json` / `add_comment_sibling_never_blocked_on_clean_json`
  (`tests/session_headless.rs`) exercising a pure `.json` with zero comments at load; rewrote
  every unit test asserting the old `supports_comments`/`enable_comments` semantics across
  `model/{json,yaml,cst_doc,any_doc}.rs`, `confy-tui/src/lib.rs`, and `confy-tui/src/tui/app.rs`
  to assert `had_comments_at_open`/no-prompt behavior instead.

### Unreleased Update — 2026-08-27T08:08:42Z
- feat(ui): comment-advisory UI rendering, web and TUI.
  - Web: a comment/trailing-comment span with `comment_advisory` set gets a red wavy underline
    (`.comment-advisory`, `text-decoration: underline wavy var(--warn)`) plus a native `title`
    tooltip (desktop hover only, matching the existing schema hover-tooltip convention). The
    Detail panel (`panel.ts`) gets a new "Note" field-label block (`web.panel.field.advisory`),
    styled like the Schema block but bordered in warn color, right after it.
  - TUI: the VALUE cell swaps its dim comment style for an underlined warn-colored one
    (`value_cell` in `overlay_detail.rs`'s sibling `ui.rs`) — the closest terminal analogue to a
    wavy underline (no hover tooltips in a terminal). The `i` Detail popup gets an appended
    `Note:` section (`detail_full_text`/`draw_detail_overlay` in `overlay_detail.rs`), independent
    of the existing `Schema:` section.
  - `RowSnapshot` (confy-tui's host-side `ViewRow` wrapper) gained `comment_advisory`, threaded
    through in `App::rebuild_rows`.
- test: added `detail_full_text_appends_note_section_for_comment_advisory` and
  `comment_advisory_renders_underlined_in_value_column` (confy-tui); fixed two `ViewRow` literals
  in `tests/serde_roundtrip.rs` missing the new field.

### Unreleased Update — 2026-08-27T07:42:41Z
- feat(host): wired `Session.strict_json` for every host. `confy-tui`'s `run()` sets it from
  the real file extension (plain `.json`, not `.jsonc`) and fires a one-shot
  `tui.host.json-comments-detected` toast when the file already had comments at open.
  `confy-ffi` gained `ConfySession::set_strict_json`/`supports_comments` wasm-bindgen methods
  (wrapped as `Session.setStrictJson`/`supportsComments` in `web/confy.ts`); `web/ui.ts`'s
  `openText` — the single entry point for desktop/URL/sample opens, shared by the Tauri host
  since it embeds the same web bundle — does the equivalent extension check and fires
  `web.host.json-comments-detected`. No `confy-tauri`-specific change needed: it has no
  Rust-side document loading of its own.

### Unreleased Update — 2026-08-27T07:32:59Z
- feat(core): added the **comment advisory** data layer — a host-supplied `Session.strict_json`
  flag (true iff the open document's real extension is plain `.json`, not `.jsonc`; confy-core
  is extension-blind, so only the host knows this) drives a new `ViewRow.comment_advisory:
  Option<String>` field, `Some(message)` when the row is a standalone comment or carries a
  trailing comment inside a `strict_json` document — non-standard JSON that confy silently
  upgrades to JSONC rather than rejecting. Distinct from schema `Violation`: this is a
  document-format note, not a JSON Schema constraint, and it's computed the same way
  (`Session::to_view_row`'s single source of truth for both `visible_rows()` and
  `view_row_at()`). Also added a general-purpose `has comment` Type Filter facet
  (`Cell::HasComment`/`TypeFilter.comment_only`, mirroring the existing `Warning` facet's
  wiring) — matches any node that is itself a standalone comment or carries a trailing
  comment, across every format (not just the `strict_json` advisory case). New i18n keys
  `core.comment.advisory`, `tui.host.json-comments-detected`, `web.host.json-comments-detected`
  (the latter two for a one-shot host `SetHostNotice` toast at file-open time — hosts wire the
  toast and `strict_json` themselves; the extension check is host-only knowledge).
- test(core): added `has_comment_facet_matches_comment_nodes_and_trailing_comment_carriers`
  to `session::type_filter::tests`; extended existing `TypeFilter::matches`/
  `is_reverse_excluded` call sites with the new `has_comment` parameter.

### Unreleased Update — 2026-08-27T07:25:59Z
- fix(schema): JSON `$schema`-hint detection (`schema::hints::detect_json`) and external
  schema-file compilation (`Session::apply_schema_text`) previously parsed with strict
  `serde_json::from_str`, which silently fails on *any* `//`/`/* */` comment anywhere in the
  text — not just before the `"$schema"` key as the old code comment claimed. A JSONC document
  with a comment anywhere would never have its schema hint detected at all (`detect_hint`
  degraded to `None` with no visible cause), contradicting the design spec's explicit
  "JSON/JSONC" scope (`docs/superpowers/specs/2026-08-10-json-schema-support-design.md` §1). A
  JSONC-authored schema file hit `load_error: "schema is not valid JSON"` the same way. Both
  now parse through the project's own lossless JSON/JSONC parser (`AnyDocument::from_str_as` +
  `ConfigDocument::to_value()`) instead of `serde_json` directly, so comments anywhere in
  either document no longer break either path. Added `schema::value_bridge::value_to_json`, a
  path-free `Value -> serde_json::Value` lowering shared by both call sites.
- test(schema): added `detect_hint_json_survives_comments_anywhere_in_the_document` and
  `session_apply_schema_text_accepts_jsonc_authored_schema` regression tests to
  `tests/schema_headless.rs`.

### Unreleased Update — 2026-08-27T05:00:00Z
- fix(core): document-level conversion (`model/convert.rs`, `confy convert`/TUI `C`) now
  carries a detected schema hint (`schema::hints::detect_hint` — JSON `"$schema"` root key,
  YAML `# yaml-language-server: $schema=` modeline, TOML `#:schema` leading comment) across
  format boundaries by *convention*, not by structure. Previously the hint's source-format
  artifact was rendered verbatim in the target: a JSON `$schema` field became a stray YAML/TOML
  data key (polluting the output and unrecognized by `detect_hint` on re-open), and a YAML/TOML
  hint comment survived only as an ordinary `//`/inline comment in a JSON target. `convert()`
  now strips the source artifact and re-authors the hint in the target's own convention;
  when the target root shape can't carry that convention (e.g. converting into a non-object
  JSON root), the hint is dropped with a warning via the existing `ConvertResult.warnings`
  path instead of silently disappearing. Hint-line recognition is centralized in
  `schema::hints` (shared by detection and conversion) and handles merged leading-comment
  blocks correctly: TOML splits only the hint's own line out of a merged comment node, and
  YAML scans every line of the leading comment run (not just the first), matching
  `detect_yaml`'s own leading-run logic.
- docs(context): added a **Schema hint** glossary entry (`docs/reference/CONTEXT.md` § Schema)
  distinguishing the format-neutral concept from its three format-specific marker syntaxes,
  and a clarifying note on the **Conversion** entry that a schema hint is re-authored in the
  target's convention rather than carried across verbatim like an ordinary comment.
- test(core): added 11 new `model::convert::tests` unit tests covering the full 6/6 directed
  format-pair matrix, a drop+warn case, a same-hint no-op regression, and three merged-leading-
  comment-block correctness cases; added
  `dispatch_convert_run_carries_toml_schema_hint_to_json` to `tests/session_headless.rs`.

### Unreleased Update — 2026-08-27T01:00:00Z
- fix(schema): `undo`/`redo` now also re-detect the in-document schema hint
  (`Session::sync_schema_hint`), matching every other mutation path. They
  previously bypassed `on_mutation_success` entirely and called only
  `revalidate_schema()`, so undoing/redoing past an edit that changed the
  `$schema`/modeline hint left the session on the stale schema until the
  next unrelated edit forced a resync.

### Unreleased Update — 2026-08-27T03:00:00Z
- fix(schema)!: `Session::sync_schema_hint` now clears the loaded schema
  when the in-document hint disappears (deleted, or edited into plain
  text), instead of leaving a now-stale schema in place. The prior
  "leave untouched" behavior (2026-08-27T00:00:00Z entry above) was
  guarding a case that turned out not to exist: the TUI's `--schema`
  CLI flag, the only way to load a schema without a matching in-document
  hint, is removed in this change (`crates/confy-tui/src/cli.rs`,
  `tui/mod.rs`) — every host now loads a schema *because* of a detected
  hint, so "no hint" can unconditionally mean "no schema".

### Unreleased Update — 2026-08-27T00:00:00Z
- feat(schema): in-document schema hints (`$schema` / YAML modeline / TOML
  `#:schema`) now reload live as the document is edited, instead of only at
  file open. `Session::on_mutation_success` re-detects the hint after every
  committed mutation and dedups against the currently loaded schema itself
  (`Session::sync_schema_hint`) — same source + prior success is a no-op,
  same source + prior failure retries, a changed source requests a fresh
  fetch. No hint detected leaves an already-loaded schema untouched. This
  replaces the VS Code extension's host-only `schemaDedup.ts`/`needsSchemaReload`
  (ADR 0007) — `SchemaSessionManager.syncSchema` now just resolves whatever
  `schema_fetch_request` the snapshot already carries — and adds the same
  live-reload wiring to the TUI's event loop; the webview/Tauri/browser
  hosts needed no changes since their render loop already checked
  `schema_fetch_request` on every snapshot.

### Unreleased Update — 2026-08-27T02:34:32Z
- fix(web): desktop Raw view no longer inflates the header/filter-row layout. The
  `#raw` `<pre>`'s own style rule was a bare `.raw-view` class selector, which
  collided with the identically-named `body.raw-view` state class `setRawView()`
  toggles for hiding the FAB; that leaked `white-space:pre` (plus `font-size`/
  `line-height`) onto `<body>` and, through inheritance, into the filter row's
  `#searchWrap`, where it turned collapsed whitespace text nodes between the
  search icon/input/clear button into rendered line breaks, ballooning the
  filter row's height. Scoped the rule to `#raw.raw-view` so it only styles the
  raw-text element; `body.raw-view` continues to work unchanged for its FAB-hiding
  rule.

## [v0.22.0] - 2026-08-27

### Unreleased Update — 2026-08-27T01:29:56Z
- docs: repo-wide documentation audit. Fixed a real bug: `scripts/sync-releases-md.sh`'s
  awk anchor match was a plain substring search over the whole file, so a version-bump
  anchor that also appears in `RELEASES.md`'s `## Details` prose bullets (e.g. "VS Code
  extension") corrupted those bullets with a trailing `|||| vX.Y.Z`; restricted the match
  to lines starting with `|` and cleaned up the two already-corrupted bullets. Applied the
  repo's existing "✅ Shipped — historical reference" banner (already on 23 of 34
  `docs/superpowers/plans/` files) to 11 more fully-shipped plans that were missing it, for
  consistency, with zero content changes:
  `2026-08-11-web-code-audit-remediation-plan.md`,
  `2026-08-17-adr-0004-unified-clipboard-targeting.md`, the five
  `2026-08-18-row-state-visual-language-phase{1..5}.md`,
  `2026-08-20-schema-warning-indicators-plan.md`, `2026-08-20-vscode-outline-provider-plan.md`,
  `2026-08-21-message-system-integration.md`, and `2026-08-21-vscode-schema-hints.md`. Added
  `status: implemented` frontmatter to ADRs 0001, 0002, 0003, and 0008 to match the
  convention already used on ADRs 0004-0007. No broken links found; all `docs/reference/*.md`
  and ADR factual claims spot-checked against current source and confirmed accurate;
  `docs/superpowers/` archive left intact (it is actively cited 26+ times from
  `docs/reference/*.md` and `docs/adr/*.md` as the permanent design-rationale record, not
  disposable scratch).

### Unreleased Update — 2026-08-27T01:12:18Z
- docs: added `docs/reference/CHROME.md` as the single source of truth for the web/touch
  header + filter-row chrome — button inventory (id/`data-act`, group, i18n key), the
  responsive fold breakpoint ladder (desktop `@media` px vs touch `@container` px, side by
  side), the per-host chrome-trimming matrix (web desktop/touch, VS Code webview, Tauri
  desktop/mobile), and a checklist for adding/moving a toolbar button. Trimmed the duplicated
  descriptions this consolidates out of `WEBUI.md` (desktop + touch responsive-toolbar
  paragraphs), `VSCODE.md` (§Chrome trimming), and `TAURI.md` (§Chrome trimming (Desktop)) —
  each now points to `CHROME.md` instead of restating it, fixing the stale claims those three
  had already accumulated (`VSCODE.md`/`TAURI.md` still described the Raw/Tree toggle as
  defaulting to the filter row, no longer true since it moved into the header). Registered
  `CHROME.md` in `docs/reference/README.md`'s index and added `toolbar-fold.ts`/`CHROME.md`
  pointers to `CLAUDE.md`'s module map.

### Unreleased Update — 2026-08-27T00:59:53Z
- docs: moved 9 root-level developer-reference docs (`BEHAVIOR_MATRIX.md`, `CONTEXT.md`,
  `MESSAGES.md`, `PORTING.md`, `ROW_STATE_MODEL.md`, `TAURI.md`, `TUI.md`, `VSCODE.md`,
  `WEBUI.md`) into a new `docs/reference/` directory, keeping `CLAUDE.md`, `README.md`,
  `CHANGELOG.md`, and `RELEASES.md` at the repo root. Added `docs/reference/README.md` as an
  entry-point index. Updated the only real path-based Markdown links (`README.md`,
  `RELEASES.md`) to the new paths; left the many bare-filename prose mentions elsewhere
  (source doc-comments, ADRs, `docs/superpowers/`) unchanged since they remain unique and
  greppable.

### Unreleased Update — 2026-08-27T00:21:19Z
- refactor(web): moved the desktop/touch Tree/Raw toggle button from the filter row
  (`#viewTabs`/`.viewtabs`) up into the toolbar header (`#editGroup`/`.edit-grp`), immediately
  left of the Info button. Filter row 2 is now `search-bar · type-filter · expand/collapse ·
  undo/redo`; header row 1 gains `… theme · lang · raw/tree · info`. The fold breakpoint moved
  with it (desktop `#viewTabs{display:none}` → `#btnViewToggle{display:none}` at the same
  ≤600px; touch `.viewtabs{display:none}` → `.edit-grp [data-act="toggleview"]{display:none}`
  at the same ≤720px), so it still folds first, same as before. VS Code and Tauri desktop hide
  the whole toolbar header (native chrome replaces Open/Save/Undo/Redo/theme/lang/info there)
  and have no native substitute for the webview's Raw view, so `main()` (`web/ui.ts`)
  reattaches `#btnViewToggle` to the end of the filter row for those two hosts at startup,
  keeping it exactly where it was before this move (touch is never hosted by either, so it
  needs no equivalent).

### Unreleased Update — 2026-08-27T00:10:47Z
- refactor(web): moved the desktop/touch Undo/Redo buttons from the toolbar header
  (`#editGroup`/`.edit-grp`) into the filter row (new `#histGroup`/`.hist-grp`), positioned
  immediately left of the Tree/Raw toggle — `search-bar · type-filter · expand/collapse ·
  undo/redo · raw-tree`. Fold-priority breakpoints (`#btnUndo`/`#btnRedo` ID selectors on
  desktop, `.hist-grp [data-act="undo"/"redo"]` on touch) are unchanged, so Undo/Redo still
  fold away last, narrowest-first. Added `body.host-vscode #histGroup` /
  `body.host-tauri-desktop #histGroup { display: none }` so the VS Code webview and Tauri
  desktop app — both of which already provide Undo/Redo through their own native entry point
  (VS Code's z/y, Tauri's native Edit menu) and previously never showed the toolbar-header
  copy either — keep not showing a filter-row copy now that the buttons live in the
  always-visible filter row. Plain browser desktop/touch and the Tauri app's touch UI now show
  Undo/Redo in the filter row.

### Unreleased Update — 2026-08-26T15:18:02Z
- docs: doc-drift audit covering the 2026-08-26 work above (per-button toolbar fold, touch
  Ctrl/Shift-tap multi-select, and the popup/`$EDITOR` trailing-comment fix) plus a re-check of
  the 14 root docs / 3 host READMEs / 8 ADRs. Found and fixed: `WEBUI.md`'s desktop and touch
  "Responsive toolbar"/"Responsive chrome collapse" paragraphs still described the old
  per-group fold (`Tree/Raw ≤600px, Expand/Collapse ≤500px, Undo/Redo/Theme ≤440px`) — rewritten
  to the actual per-button breakpoint ladder now in `web/style.css`/`web/touch/style.css`;
  `WEBUI.md`'s touch Gesture→Intent map was missing the Ctrl/⌘-tap and Shift-tap multi-select
  entry entirely — added; `docs/adr/0006-outline-symbol-representative-span-anchoring.md`'s
  `status:` frontmatter was still `accepted` though the anchoring policy it defines has been
  load-bearing production code since the day it was written (2026-08-20) and is cited by name
  in cst_project.rs/node.rs/cst_edit/ and the 2026-08-26 breadcrumb-chain fix — corrected to
  `implemented (2026-08-20)`, matching how ADR 0004/0007 were corrected in the 2026-08-23 audit.
  The trailing-comment fix needed no doc changes: `BEHAVIOR_MATRIX.md` §6.3's "captures just the
  edited node" and `TUI.md`'s comment section already describe the intended behavior the bug
  broke, not the bug itself. **Flagged, not auto-fixed** (a version/release-process call, not a
  doc-accuracy one): `editors/vscode/package.json` reads `0.21.1` (bumped in `b9ae6b4`, no
  matching `chore: release`) while `Cargo.toml`/`web/package.json`/`RELEASES.md` all say
  `0.21.0` — needs a maintainer decision on whether to revert to `0.21.0` pending the real next
  release or leave it, before it hits `release.yml`'s version-consistency gate.

### Unreleased Update — 2026-08-26T00:00:00Z
- fix(vscode): native text-editor `DocumentSymbolProvider` now expands each parent symbol's editor-facing `range` to include all descendant symbol ranges (while preserving the core `text_range` anchoring policy from ADR 0006). This restores VS Code breadcrumb parent-chain resolution for TOML nested tables like `[workspace.package]` when parent/child source spans are non-enclosing by design.
- test(vscode): added an extension-host integration regression (`editors/vscode/test-integration/suite/index.mjs`) plus fixture (`workspace-package.toml`) asserting the `workspace` symbol range contains its `package` child range.
- feat(web): desktop/touch header toolbar responsive fold is now per-button instead of per-group — `web/style.css` (`@media`) and `web/touch/style.css` (`@container`) previously collapsed `#navGroup`/`#editGroup` (`.nav-grp`/`.edit-grp`, 2 and 5 buttons respectively) into the "⋯ More" menu all at once at a single breakpoint each. Each button now has its own breakpoint and folds one at a time, right-to-left in priority order (Collapse all → Expand all → Help/About → Language → Theme → Redo → Undo), giving finer-grained use of available header width. Pure CSS change — `web/toolbar-fold.ts`'s `foldedEntries`/`isFolded` mechanism already operated per-button (by element id / `data-act` selector), so no JS/registry changes were needed; `web/toolbar-fold.spec.mjs` parity checks pass unchanged.
- feat(web): touch UI now supports Ctrl/⌘+tap and Shift+tap multi-select on a row body, reusing the desktop `resolveClick` gesture resolution (`web/select.ts`, previously desktop-only) — Ctrl/⌘-tap toggles a row into/out of the selection, Shift-tap ranges from the last plain/Ctrl-tap anchor, matching desktop's `onTreeClick` exactly. Only reachable on touch+keyboard hybrids (iPad+trackpad/keyboard, Surface, Chromebook, touchscreen laptops) since `PointerEvent.ctrlKey/shiftKey/metaKey` reflect real held keys and are unset on pure touch devices — a plain tap is unaffected (still a single-row `SetSelection`). `web/select.ts`'s internal `Mods` interface is now exported for reuse. `web/touch/app.ts`'s `openText` also calls `resetAnchor()` on document swap, mirroring desktop, so a stale shift-range anchor can't survive an Open.
- test(web): extended `web/touch-pointer-slot.spec.mjs`'s extracted-`handleTap` coverage with Ctrl-tap toggle and Shift-tap range assertions, and updated its structural checks for the new `handleTap(target, row, clientY, mods)` signature and `pointerup` call site; updated `web/touch-paste-drag.spec.mjs`'s structural regex for the same call-site change.

### Unreleased Update — 2026-08-26T12:01:31Z
- fix(core): editing a trailing `#`/`//` comment via the popup editor (Web) or `$EDITOR` (TUI) on a TOML or JSON scalar/array-element/inline-table value was silently discarded — `Mutation::Replace`'s TOML `replace_value()` (`crates/confy-core/src/model/cst_edit/replace_delete.rs`) unconditionally kept the pre-edit trailing comment, a behavior designed only for the inline single-line editor's separate `SetTrailingComment` staging (`session/inline_edit.rs`), which the external-edit path never populates. `replace_value` now returns the edited fragment's own trailing comment (if any) and `cst_edit::apply()` applies it via `set_trailing_comment`; JSON's `serialize_fragment`/`fragment_of` (`crates/confy-core/src/model/json/edit.rs`) now also include the node's trailing comment in the text shown to the user (previously omitted entirely), and its `replace()` applies an edited one the same way. YAML was already correct (whole-entry `Replace`) and needed no change.
- test(core): added `replace_scalar_applies_edited_trailing_comment` / `replace_inline_table_value_applies_edited_trailing_comment` (TOML), `fragment_of_member_includes_trailing_comment` / `replace_member_applies_edited_trailing_comment` / `replace_member_without_comment_keeps_old_comment` (JSON), `replace_map_entry_applies_edited_trailing_comment` (YAML regression guard), and an end-to-end `dispatch_external_edit_applies_edited_trailing_comment` in `tests/session_headless.rs` driving the real `Intent::BeginEditExternal` → `Intent::ApplyReplace` pipeline.

### Unreleased Update — 2026-08-26T13:17:44Z
- fix(core): the JSON trailing-comment fix above broke every JSON `Replace` whose fragment carried a `//` comment with no trailing newline — exactly the shape `serialize_fragment` actually produces for the popup editor / `$EDITOR` (no newline is appended after the comment). `parse_member_fragment`/`fragment_member_trailing_comment`/`fragment_element_trailing_comment` (`crates/confy-core/src/model/json/edit.rs`) wrap the fragment as `{fragment}`; without an intervening newline, the `//` line-comment consumed the synthetic closing `}` too, so the member reparse failed, fell back to treating the whole `"key": value // comment` fragment as a bare value, and surfaced as `invalid JSON: unexpected \`:\` (COLON) after document`. The three wrap sites now insert a newline before the closing brace.
- test(core): added `replace_member_applies_edited_trailing_comment_no_source_newline`, mirroring the real `serialize_fragment` output shape (comment present, no trailing `\n`), to guard this regression.

## [v0.21.0] - 2026-08-24

### Fixed
- fix(release): `editors/vscode/package.json` was missed during the version bump (still `0.20.0`), failing `release.yml`'s version-consistency gate against tag `v0.21.0`. Bumped to `0.21.0`.

### Unreleased Update — 2026-08-24T09:15:00Z
- docs: follow-up doc audit (one day after the 2026-08-23 repo-wide pass) covering today's
  schema-load-error/touch-keyboard/FAB/warning-marker work: `WEBUI.md` gained the missing
  Touch UI writeup for the external-keyboard shortcut parity shipped in the
  2026-08-24T08:30:00Z entry above; `MESSAGES.md` §2 now documents the two new
  `web.host.schema.load-error`/`tui.host.schema-load-error` host-authored `Warn` keys
  (shipped in the 2026-08-24T08:00:00Z entry above) and clarifies its "42 keys" count is
  `core.*`-only (the `severity_of_covers_the_full_catalog_table` test's own scope), unaffected
  by new host keys; `docs/adr/0004-unified-clipboard-move-targeting.md` and
  `docs/adr/0007-vscode-schema-session-in-place-replace.md` had stale `status:` frontmatter
  (`accepted`/`proposed`) corrected to `implemented` — both decisions shipped (v0.20.0 and
  2026-08-21 respectively) and were already being cited elsewhere as settled. No other doc
  drift found across the 14 root docs, 3 host READMEs, and 8 ADRs.

### Unreleased Update — 2026-08-24T08:30:00Z
- feat(web/touch): touch UI (`web/touch/app.ts`) now supports the full desktop keyboard-shortcut set (`web/key-intent.ts`'s `resolveKeyIntent`, shared verbatim — no changes to it or to desktop `web/ui.ts`) for external/Bluetooth-keyboard users on tablets, wired via a single new `document.body` `keydown` listener (`onKey`), guarded against focused `INPUT`/`TEXTAREA`/`SELECT` fields and the URL/external-edit sheets. Navigation (j/k/g/G, arrows, Home/End, Shift+↑/↓ range-select), edit actions (a/d/c/x/v/r/s), expand/collapse (1/2/0/9), Nudge (+/-), `/` focus-search, `f`/`C` TypeFilter/Convert (already core-mode-driven and reactively rendered on touch), `?` Help, Ctrl+S/Ctrl+O save/open, z/y undo/redo, and Space multi-branch toggle (new `toggleSelectedBranches`, mirroring desktop's) all dispatch through the same core `Intent`s desktop uses. Two intents are host-specific because touch's own editing/kind-switch surfaces bypass the core sub-modes those intents drive on desktop (`Mode::Edit`, `Mode::KindSwitch`, neither of which touch renders): `e`/`BeginEdit` and `K`/`OpenKindSwitch` now open touch's existing panel/kind sheets instead. `i`/Enter (`ToggleDetail`) toggles the host-local detail sheet directly (no core mode backs it on touch), and `Escape` closes that sheet first if open. `q`/`QuitRequested` is suppressed entirely (`vshost: true`) — a web/touch surface has no "quit" concept. Verified via `npx tsc --noEmit`, `node build.mjs`, and the full `node run-tests.mjs` suite (all 23 spec files pass unchanged).

### Unreleased Update — 2026-08-24T08:00:00Z
- feat(schema): schema-file load failures (`$schema` local path not found / URL fetch failed) now surface a user-visible warning on every platform, not just the VS Code extension's Problems-panel diagnostic. Web desktop (`web/ui.ts`) and touch (`web/touch/app.ts`) dispatch a new `web.host.schema.load-error` host notice (severity `Warn`, matching VS Code's `DiagnosticSeverity.Warning` and `core.schema.violation`'s tier) once `resolveSchemaFetchRequest` resolves with `schema_status.load_error` set, shown via the existing toast/status-line mechanism (Tauri desktop inherits this for free — it embeds the same `web/` UI). The TUI (`crates/confy-tui/src/tui/mod.rs`) now dispatches a `tui.host.schema-load-error` notice on startup when its one-shot `resolve_schema_source`/`apply_schema_text` call fails, surfacing it in the status line instead of silently dropping the error. Both new catalog keys added to `i18n/en.json`/`i18n/zh-TW.json` and classified `Warn` in `confy-core`'s `notice::severity_of` table.

### Unreleased Update — 2026-08-24T07:14:01Z
- fix(core): schema-driven editing hints (enum picker, detail-panel "general info" line) now resolve correctly under a root/branch keyed by `patternProperties` (the common "dictionary of named objects" schema idiom, e.g. a `tasks.toml`/`tasks.yaml` map keyed by arbitrary task names) — `hints_edit.rs::resolve_subschema` only ever walked `properties`/`items`, so any path whose first segment matched a `patternProperties` regex instead of a literal `properties` key resolved to `None` immediately, silently dropping every hint below it (an `enum`-constrained field like `put_in_key.type` never opened the enum popup, and the detail panel showed no schema info) even though `validate.rs`'s full `jsonschema`-crate validator — a separate code path — already understood `patternProperties` fine, so violation warnings kept working and masked the gap. Added a `pattern_property_match` fallback (new direct `regex` dependency, already present transitively via `jsonschema`) tried when no literal `properties` key matches. New `schema_headless.rs` cases cover both the match and no-match branches for `resolve_edit_hint`/`resolve_schema_info`. Verified against the real `/tmp/verify-test/tasks.schema.json` + `tasks.toml` fixture: `put_in_key.type` now resolves to `EditHint::Enum(["command","script","script_sudo","scp"])` and surfaces its description/type info, instead of `None`/`None`.

### Unreleased Update — 2026-08-24T06:29:42Z
- fix(core): a scalar reached through a `Key` nested under an array index in JSON (e.g. `tasks[0].name`) now edits inline instead of always opening `$EDITOR`/the popup editor — `JsonDocument`'s member lookup (`json/project.rs::walk`'s recursive `JsonIndex`) already `Replace`/`Rename`-addressed such a node precisely regardless of nesting depth, but `Session::edit_target_kind()` routed it externally anyway because it read the same `array_elements_addressable` facet that also (correctly) governs whether the array *element itself* needs external-edit wrapping (`scalar_fragment`) — a JSON-specific need unrelated to nested-member addressability. Split the two concerns into a new `ConfigDocument::array_member_keys_addressable()` facet (defaults to mirroring `array_elements_addressable`; `JsonDocument` overrides it to `true` while keeping `array_elements_addressable` `false`), and pointed `edit_target_kind()`'s nested-`Key` routing at the new facet. `crates/confy-core/src/model/{document,any_doc,json/doc}.rs`, `crates/confy-core/src/session/session.rs`. Verified via `cargo test -p confy-core --lib` (514 passing) and `cargo test -p confy-tui --lib` (198 passing, including the new `json_key_through_array_index_edits_inline` regression and the pre-existing `json_array_element_external_edit_replaces_only_that_element` wrap-precision test), plus a standalone repro against a real `tasks[0].name` fixture confirming `Inline` routing and a single-field commit.

### Unreleased Update — 2026-08-24T05:48:48Z
- style(web/touch): touch's leaf-row layout (`web/touch/render.ts`) now matches desktop's field order exactly — `key = value type·note #comment` — instead of putting the trailing comment before the kind badge (`web/render.ts` has always ordered them this way). Also ported desktop's `.kind-note` treatment: the kind pill's notation suffix (the `·dec`/`·scope`/`·"…"` after the type label) is now its own `<span class="kind-note">` dimmed to `opacity:.6` (`web/touch/style.css`), rather than rendering at the same brightness as the type label. `kindBadgeText` (plain string) became `kindBadgeHTML` (returns the same markup desktop's `renderKindBadge` produces) so both surfaces share one visual language for the badge, continuing to draw from the already-shared `kindLabelParts` (`web/kind-labels.ts`). Branch-row order (`key`, count, kind, comment) was already correct on touch and is unchanged. Verified via `npm run typecheck && npm run build && npm test` and a headless-Chrome inspection of `touch.html?ui=touch` confirming both the new field order and a computed `.kind-note` opacity of `0.6`.

### Unreleased Update — 2026-08-24T05:35:03Z
- style(web): the schema-warning indicator (desktop `web/render.ts` / touch `web/touch/render.ts`) switched from a CSS circle dot to an inline-SVG triangle (`▲`/`△`), matching the TUI's glyph exactly instead of diverging (circle on web, triangle in the TUI — an accidental split from each surface's independent history, not a deliberate design decision; see chat for the git-archaeology). New `IC_WARN_FILL`/`IC_WARN_HOLLOW` constants render a filled or stroke-only `<polygon>`; `.warn-dot-fill`/`.warn-dot-hollow` CSS now target the `polygon` (`fill`/`stroke`) instead of the element's own `background`/`border`. A CSS border-trick triangle was considered and rejected (no clean way to draw the hollow variant). Verified via `npm run typecheck && npm run build && npm test` and headless-Chrome screenshots of both `index.html` and `touch.html?ui=touch` against the built-in `schema` sample (hollow △ on the branch, filled ▲ on its two violating children).

### Unreleased Update — 2026-08-24T04:06:48Z
- fix(tui): added one space between the schema-warning marker and the key text (`crates/confy-tui/src/tui/ui.rs`) — the marker previously sat flush against the key with no breathing room. `KEY_X` moves from column 6 to 7 to account for the new spacing column; updated its doc comment and the other stale layout comment that referenced the old column. Verified on the real TUI binary against a `[server]\nport = "nope"` fixture with a `server.port: integer` schema.
- feat(web): the schema-warning dot (desktop `web/render.ts` / touch `web/touch/render.ts`) is no longer a fixed-position `::after` pseudo-element — it is now a real element rendered in the row's own flex flow immediately after the caret, so its column tracks each row's indentation exactly like the TUI marker. It also gains the same hollow/filled distinction: `.warn-dot-fill` (solid) for a row whose own `violations` is set, `.warn-dot-hollow` (ring only) for a branch that merely has a violating descendant (`has_descendant_violation`) — a branch that is both itself invalid and has invalid descendants shows filled, matching the TUI's "own problem outranks the summary" rule. This closes the scope gap explicitly left open by the previous entry's plan (desktop/touch dot was deliberately left as the old fixed-position filled-only dot; user feedback confirmed the original request covered both TUI and web). Verified via `npm run typecheck && npm run build && npm test` (all existing specs, including the `warn-branch`-class assertions in `render.spec.mjs`/`touch-render.spec.mjs`, still pass unchanged) and headless-Chrome inspection of both `index.html` and `touch.html?ui=touch` against the built-in `schema` sample (hollow dot on the branch, filled dots on its two violating children, each indented one level deeper than its parent's dot).

### Unreleased Update — 2026-08-24T03:44:51Z
- feat(tui): the schema-warning marker moved out of the fixed left gutter (buffer column 1) to sit immediately left of the key text, so its column now tracks each row's indentation instead of staying pinned at the same spot regardless of depth (`crates/confy-tui/src/tui/ui.rs`). Also split the single filled `▲` into hollow `△` for a branch that only *summarizes* a violation somewhere in its subtree, vs filled `▲` for a row that violates itself — a branch that both violates and has violating descendants still shows filled, since its own problem outranks the summary. `KEY_X` stays column 6; updated its doc comment and a second stale comment that still described the old (pre-warning-marker) 5-column layout. Rewrote `branch_with_descendant_warning_shows_marker_glyph_regardless_of_expand_state` for the new hollow/filled split (collapsed: `△` present, no `▲`; expanded: 2 hollow, 1 filled) and verified on the real TUI binary against a `[server]\nport = "nope"` fixture with a `server.port: integer` schema.
- feat(web): desktop gains the touch UI's bottom-right floating add/paste (FAB) button pair (`web/index.html`, `web/ui.ts`, `web/style.css`), sharing its glyphs, markup, and add/paste decision logic with touch via a new `web/fab.ts` (touch's `touch/app.ts` now delegates to it instead of keeping its own copy). Desktop's FAB hides in Raw view via a new `body.raw-view` class toggled from `setRawView`. New `web/fab.spec.mjs` pins `fabAddAction`/`fabHTML`'s pure logic. Verified via `npm run typecheck && npm run build && npm test` (613 checks passing) and headless-Chrome interaction on both `index.html` and `touch.html?ui=touch` (FAB add/paste/clear, Raw-view hide/show).
- fix(web/desktop): the schema-violation row frame switched from a solid inset `box-shadow` ring to a dashed `outline`, matching touch's existing dashed frame (`web/style.css`) — an `outline` also composes with `.row.drag-over-into`'s green drop ring instead of overwriting it, so both render simultaneously when dragging a node onto a violating row.

### Unreleased Update — 2026-08-24T01:57:14Z
- fix(web/touch): a cut/paste (or move-grip) failure's error toast kept "re-popping" on every subsequent unrelated interaction — selecting a different (valid) node, toggling expand, dragging the move grip — until the paste actually landed or the clipboard was cleared. Root cause: `Session.notice` deliberately persists across pure navigation intents (`MESSAGES.md` §1.1 — cursor move/`ToggleExpand`/`SetCursor`/`SetSelection` never clear it), but touch's `render()` calls `renderNotice(snap.notice)` on every dispatched `Intent`, and `renderNotice` unconditionally replayed the toast's show animation and restarted its 3s/1.6s auto-hide timer on each call — so any re-render while the same stale notice was still sitting in the slot made a single error look stuck in a loop. TUI/desktop don't show this: TUI's status line is static (undrawn) text with no timer, and desktop's `#error` element is a persistent non-animated node, not a timed toast. Fixed by tracking the severity+text of whatever the toast last actually showed (`web/touch/app.ts`'s `renderNotice`) and skipping the animation/timer replay when an incoming notice is identical to what's already displayed; a genuinely new or changed notice (including the same message reappearing after the slot was cleared) still shows normally. Added `web/touch-notice-toast.spec.mjs`, which reproduces the exact regression against the pre-fix source and passes against the fix.

### Unreleased Update — 2026-08-23T11:20:00Z
- docs: repo-wide doc/memory audit (CLAUDE.md/CONTEXT.md/TUI.md/WEBUI.md/MESSAGES.md/README.md/RELEASES.md/VSCODE.md/TAURI.md/PRIVACY.md/ROW_STATE_MODEL.md/BEHAVIOR_MATRIX.md/PORTING.md + all 8 ADRs, plus a codebase-memory-mcp index-health check) found the set overall accurate and cross-reference-clean; fixed the handful of drifted spots: `CLAUDE.md`'s module map still described `yaml/edit.rs` as one file (it was split into `yaml/edit/{mod,block,flow,mutations,convert,resolve}.rs` in the same 2026-08-11 audit-remediation pass that split `cst_edit/`) and omitted `model/text_range.rs` entirely; `TUI.md` §Filter stated a scalar's value is "never matched" by the fuzzy filter, contradicting both the actual `haystack()`/`recompute_filter` implementation and `WEBUI.md`'s (correct) description; `ROW_STATE_MODEL.md`'s `session.rs` line-number citations for the clipboard-armed guards and `selected_paths()` had drifted from refactors; `RELEASES.md` had stray `||||` pipe-artifact suffixes on two Details bullets; `docs/adr/0006`'s `status: proposed` was stale — its anchoring decision already ships (VS Code outline/breadcrumb symbols) — corrected to `accepted`. No content was deleted; all decisions/rationale kept, only factual/citation drift corrected.

### Unreleased Update — 2026-08-23T10:15:00Z
- fix(web/desktop): the Schema card now has proper vertical spacing above it, separating it from the `Sign` row it follows. The shared `.detail-body dl` rule (also used by an unrelated read-only tree-hover preview) zeroes the `<dl>`'s margin; desktop's touch counterpart absorbs that via its own `.detail dl` bottom margin, but desktop's `#detailBody` has none, so the Schema field-label (margin-bottom only) sat flush against the `dl`'s last row with no gap. Added a targeted `#detailBody dl + .field-label { margin-top: 12px }` — this selector only ever matches the Schema label (the sole field-label that follows a `dl` in `panelHTML`'s output), so it doesn't touch any other field's spacing. Touch was already correct, unaffected.

### Unreleased Update — 2026-08-23T09:00:00Z
- fix(web): the shared detail/edit panel's `Schema` card now renders right after Meta (Path/Children/Sign), **before** the Actions row (`web/panel.ts`), instead of after — so Copy/Cut/Delete/External-edit stay the panel's fixed trailing element regardless of whether a row carries schema info. Also dropped the negative-top-margin hack both stylesheets used to tuck the card under the old post-Actions position (`margin: -4px 0 …`) in favor of the plain bottom-margin-only spacing every other panel field uses (`margin: 0 0 …`, matching `.kindbtn`/`.v-edit`), and fixed the touch sheet's card padding (`10px 12px` → `12px 14px`) to match its own `.preview` card exactly (desktop's already matched). `web/panel-schema.spec.mjs`'s order assertion flipped to expect Schema before `row-btns`.

### Unreleased Update — 2026-08-22T14:30:00Z
- feat(core+tui+web): unified schema info into the detail panel across all platforms, and closed a gap where only `enum`/`const`/numeric-bounds constraints or an active violation ever showed anything — the common plain-typed case (e.g. `{"type":"string","description":"…"}`, no `enum`/bounds) surfaced nothing at all outside a violation. New `crates/confy-core/src/schema/hints_edit.rs::resolve_schema_info` + `Session::schema_info(path)` (orthogonal to `edit_hint`/`EditHint` — that resolves a picker *widget*, this reads `description`/`type`/`format`/`pattern` straight off the same resolved sub-schema, purely for display) close it. The TUI's `i` Detail popup (`overlay_detail.rs`) and the shared web/touch/VS Code-webview `panelHTML` (`web/panel.ts`, via a new `session.schemaInfo(path)` FFI binding + `Session.schemaInfo()` wrapper in `web/confy.ts`) now render a trailing `Schema` section combining all three independent sources — non-widget info, constraint description, violation message(s) — whichever apply, omitted only when none do. Web's Schema field is also now a bordered card (mirrors the panel's existing `.preview` box language, matching every other field's boxed style instead of reading as bolted-on text) whose border tints `--warn` when the row has a violation, reusing the tree row's own `.row.schema-violation` warn signal instead of a second one. New i18n key `web.panel.field.schema` (en/zh-TW, added in the initial pass). Verified on the real TUI binary: a `host` field with only `type`/`description`/`format` (no enum/bounds, no violation) now shows its Schema section; previously blank. Docs: `TUI.md` § Status & diagnostics, `WEBUI.md` § Shared edit/detail panel, `CLAUDE.md` schema module map.

### Unreleased Update — 2026-08-22T12:37:41Z
- refactor(web): desktop's per-row reorder grip moved from its own left-side slot into `.row-actions`, replacing the standalone `＋` add button (its "Add child"/"Append sibling" were already reachable via the `⋮` context menu, so the button was redundant) — aligns desktop's flush-right row-actions layout with touch's. As a side effect the grip now vanishes with `⋮` whenever the clipboard is armed (`.paste-mode .row-actions{display:none}`), instead of staying visible outside that hidden group.
- fix(web/touch): the reorder grip is now hidden outright (`.app.paste-mode .drag-handle{visibility:hidden}`) while the clipboard is armed, instead of staying visible and rejecting a tap with an `action-locked` toast — removed the now-unreachable `clipboard_count` guards in `touch/app.ts` (`startReorder`, grip pointerdown) accordingly. Documented as an explicit exception to ADR 0005 §4's "toast, not silent no-op" rule (`docs/adr/0005-row-cursor-selection-clipboard-state-model.md`), matching the hide-outright pattern desktop's row-actions already used.

### Unreleased Update — 2026-08-22T01:00:00Z
- fix(tui+web): full-audit follow-up — three more §5.1/§5.2/§5.3 spec deviations found by re-verifying the whole message-system-integration branch against its design spec, all sharing the same root cause as the two fixes above (tests assert `Notice` state, never rendered presentation): (1) TUI `draw_status` rendered every non-`Error` `Notice` (`Success`/`Warn`/`Info`) in the same hardcoded white, ignoring §5.1's Success=green/Warn=yellow/Info=default table in both the default and `FilterResults`-mode branches — added a `notice_color(Severity) -> Color` helper (`ui.rs`) applied in both places; (2) desktop `web/ui.ts`'s schema-violation-count status append was a hand-rolled `` `${n} schema warnings` `` string bypassing i18n entirely (§5.3 required `core.schema.count`, which TUI's equivalent already used correctly) — switched to `tArgs("core.schema.count", …)`; (3) desktop's Success toast never auto-hid (§5.2: "1.6 s auto-hide, same animation as touch") — it just stayed visible until superseded by another notice — added a `toastT` timer (mirroring touch's `toastT` pattern) and gave `#toast` the same opacity/transform/visibility transition as touch's `.toast`/`.toast.show`, switching its show/hide mechanism off the shared `.hidden` utility class onto its own `.show` class (`index.html`, `style.css`, `ui.ts`). Each fix reproduced against the real binary/bundle first (temporarily re-applied the bug, confirmed a new regression test fails, then restored the fix and confirmed it passes): added `notice_severity_drives_status_line_color` (`ui.rs`, TestBackend cell-color assertions) and extended `render-notice.spec.mjs` with structural checks for the schema-count key and the toast timer/animation. `cargo test -p confy-tui` (211/0), `cargo test -p confy-core` (727/0), `node run-tests.mjs` full suite green (0 failures), `tsc --noEmit` clean on both `web/` and `editors/vscode/`.

### Unreleased Update — 2026-08-22T00:00:00Z
- fix(tui): a `Warn`/`Info`/`Success` `Notice` (e.g. `core.clipboard.action-locked`, surfaced when attempting a disallowed action — delete/rename/edit/move — while the clipboard is armed) was silently hidden by the status line's "clipboard armed" sticky hint (`draw_status`, `ui.rs`) and by the equivalent branch in `FilterResults` mode, both of which checked `session.clipboard` before `session.notice`. Reordered both checks so a pending notice always wins, mirroring the existing Edit-mode override and the Error-severity "never hidden" invariant (`docs/superpowers/specs/2026-08-21-message-system-design.md` §5.1's documented `draw_status` priority). Reproduced and verified fixed on the real TUI binary (cut a node, attempt `d`/Delete while armed — the warning now shows instead of the clipboard hint).

### Unreleased Update — 2026-08-22T00:30:00Z
- fix(web): `Warn`/`Info`/`Success` `Notice` text landed in `#status` correctly but was visually indistinguishable from idle status text — `web/style.css` had zero rules for the `sev-info`/`sev-success`/`sev-warn` classes `renderNotice()` applies, and no rule at all for `#toast` (Success's toast popup rendered as an unstyled, unpositioned `<div>`). Added `.footer .status.sev-warn`/`.sev-success` color rules (reusing the existing `--warn`/`--drop` tokens) and a floating, positioned `#toast` style. Also migrated the 10 desktop `web/ui.ts` call sites (undo/redo, add-child, row context menu, kind badge, edit-cell, kind popover open, kind-menu no-options, type-filter toggle, right-click context menu) that bypassed the Notice pipeline entirely via a direct `setStatus()` call — none of them dispatched `Intent::SetHostNotice`, so they carried no severity styling and never reached the diag ring. Migrated to `send({ SetHostNotice: { key, args: [], source: "host-web" } })`, mirroring touch's existing pattern; one site's hardcoded English string (`"no kind conversions for this node"`) replaced with the existing `web.host.kind.no-options` catalog key. Updated `web/modal-lock.spec.mjs`'s 5 structural assertions to match. Full web suite green (0 failures).

### Unreleased Update — 2026-08-22T00:15:00Z
- docs: added `MESSAGES.md` — a comprehensive, repo-root reference for the Notice/diagnostics message system: the Notice/Prompt-question/DiagEvent channel model, the full `severity_of` classification table and catalog key-prefix conventions, host-authored notices via `Intent::SetHostNotice`, the diagnostics ring's five event kinds and three export surfaces, and a per-host channel/behavior/rendering comparison (TUI `draw_status` priority, Web desktop's toast+status-bar/click-to-clear-error split, Touch's single-toast severity styling, the VS Code extension host's permanent native-popup carve-out, and the CLI's Notice-free `cli.*` catalog scope). Cross-linked from `CONTEXT.md`, `TUI.md`, `WEBUI.md`, and `CLAUDE.md`'s module map.

### Unreleased Update — 2026-08-21T21:45:00Z
- feat(messages): unified message system across core and all hosts (TUI, Web desktop, Touch, CLI) — replaces the legacy dual-bucket `status`/`error` model with a typed single-slot `Notice` (`Severity`, `NoticeSource`, localized text) with severity derived from a centralized `severity_of(key)` table (`notice.rs`) rather than specified at call sites. Host notices route uniformly through `Intent::SetHostNotice { key, args, source }` (`dispatch.rs`), preserving `dispatch` as the sole mutation entry point.
- feat(diag): in-Session developer diagnostics ring buffer (`diag.rs`, capacity 256, monotonic `seq`) tapping every notice assignment plus dispatch, mutation, schema, and convert events (ADR 0008). Exported via TUI `~` read-only overlay (`overlay_diag.rs`), FFI `diag_log()` / `ConfySession.diagLog()`, and web `?diag=1` console drain diffed by `seq` (`web/ui.ts`).
- feat(tui): severity-driven status line rendering (`draw_status`), giving Error notices absolute red-background priority while rendering non-Error notices in the standard status slot; `~` opens the modal diagnostics overlay; prompt overlay consumes core-rendered `prompt_question`.
- feat(web+touch): unified `#toast` element and severity-driven surface rendering on desktop (`renderNotice` in `ui.ts`) and touch (`touch/app.ts`); migrated touch `toast()` call sites to severity-driven `SetHostNotice`; deleted prompt fallback chain (`web/prompt.ts`) in favor of core-rendered `ModeView::Prompt.question`.
- feat(cli): routed all CLI convert subcommand output strings and error diagnostics through the `cli.*` i18n catalog with `--lang` resolution from config.
- refactor(core+web): removed deprecated `SessionSnapshot.status` and `SessionSnapshot.error` fields (paired core+web cutover), switched web types and components to consume `SessionSnapshot.notice` exclusively, and renamed `has_descendant_warning` to `has_descendant_violation`.

### Unreleased Update — 2026-08-21T15:05:00Z
- docs(design): closed two gaps flagged at the end of the second spec review — (1) §2 now states explicitly that host notices resolve severity through the same `severity_of(key)` table as core notices (no explicit-severity variant exists), so a host message without a catalog key can't migrate; (2) Phase 1's verification (§8) gains an explicit slot-occupancy test — a `Warn` populating `notice` while `error_text()` is `None`/`status_text()` is `Some` — since the `error_text()`/`status_text()` helpers alone (§12 Q7) only preserve old two-bucket assertions and never exercise the new single-slot case.

### Unreleased Update — 2026-08-21T14:40:00Z
- docs(design): second spec review of the message-system design (`docs/superpowers/specs/2026-08-21-message-system-design.md`) — an 8-question grill targeting single-source-of-truth and boundary concerns the first review missed; all recommendations accepted and recorded in new §12. Key amendments: severity moves out of call sites into one `severity_of(key)` table in `notice.rs`; host notices arrive as `Intent::SetHostNotice { key, args, source }` rather than a bespoke `set_host_notice` setter, keeping `dispatch` the sole mutation path (ADR 0003's boundary) and `Session.lang` the single language authority; the diag ring taps *every* notice (kind `host_notice` → `notice`, ADR 0008 amended); `NoticeSource` is confirmed developer-facing and never rendered; the VS Code extension-host carve-out is written into §9 with its costs; the 82 existing Some/None `status`/`error` assertions migrate through test-only `error_text()`/`status_text()` helpers instead of hand translation; and §5.2's touch-toast claim is corrected from "36 sites, 24 duplications" to 38 sites split 17 deleted / 7 host-notice / 14 migrated — the 7 guard host operations that dispatch no intent, so a flat delete would have shipped silence.
- docs(context): glossary sharpened — **Notice** provenance marked developer-facing and never rendered; **Severity** gains a schema-**Violation**-is-not-a-`Warn` disambiguation; **Diagnostic event** records every Notice.

### Unreleased Update — 2026-08-21T13:20:00Z
- docs(design): spec-reviewed the 2026-08-21 message-system design (`docs/superpowers/specs/2026-08-21-message-system-design.md`) via a 15-question grill; all recommendations accepted and recorded in new §11 — key amendments: SetLang-clears-notice flagged as *new* behavior (today `set_lang` clears nothing), `has_descendant_violation` rename moved to Phase 3's paired commit, `web.prompt.confirmFallback` added to the fallback-chain deletions, touch host-local messages (Firefox-iOS hint included) migrate through FFI `set_host_notice`, web `?diag=1` drain diffs by `seq`. Glossary: `CONTEXT.md` Notice entry gained its NoticeSource provenance sentence; new **Prompt question** entry.

### Unreleased Update — 2026-08-21T12:30:00Z
- docs(vscode): documented the custom editor's local/remote `$schema` loading (`31f86ba`) — `VSCODE.md` § Message protocol now lists the `read-schema-file`/`schema-file`/`schema-file-error` and `read-schema-url`/`schema-url`/`schema-url-error` message pairs (webview has no fs access; CSP blocks external fetches; host reads/fetches instead) with the webview↔host branching notes, plus brief mentions in `README.md` and `editors/vscode/README.md`.

### Unreleased Update — 2026-08-21T03:55:00Z
- fix(vscode): extension-host wasm loading is now robust across CJS/ESM boundaries. Instead of static bundle-time import of `media/pkg/confy_ffi.js`, `wasmSession.ts` resolves it at runtime from `context.extensionUri` via `pathToFileURL(...)` and then initializes from raw `.wasm` bytes. This removes the build-time `import.meta` warning and fixes the extension-host runtime `LinkError` (`Import "./confy_ffi_bg.js" ... requires a callable`) observed after artifact refresh.
- fix(web+vscode): `web/build.mjs` now assembles a fresh runtime `web/dist` on every build (including `dist/pkg/*`) through `web/assemble-dist.mjs`, so `editors/vscode/build.mjs` always stages current wasm/glue artifacts into `media/` instead of potentially stale outputs.
- test(vscode): added extension-host integration tests (`editors/vscode/test-integration/*`, `npm run integration-test`) using `@vscode/test-electron` to assert native text-editor behavior programmatically (DocumentSymbolProvider non-empty, schema diagnostics present, hover schema hints available), preventing future silent regressions.

### Unreleased Update — 2026-08-21T01:19:46Z
- feat(vscode): native TOML/YAML text editors now surface confy-core's JSON Schema support directly — Problems-panel diagnostics (schema violations, always `Warning` severity per the Soft-constraint principle, plus a load-error notice) and hover tooltips (enum/const/bounds at the cursor's node), driven by one persistent `ConfySession` per open document (`Intent::ApplyReplace{path:[],text}` in place of a per-edit rebuild, ADR 0007) and a new `Session::schema_violations()`/`Intent::DetectSchema` core surface. Defers to `tamasfe.even-better-toml`/`redhat.vscode-yaml` when installed. Scoped to VS Code's native editor only; confy's own custom editor tab is unaffected.

### Added
- feat(release): `release.yml` gains a `verify-versions` job that runs before any platform build on a real tag push — checks `Cargo.toml`, `web/package.json`, and `editors/vscode/package.json` versions plus a `CHANGELOG.md` `## [vX.Y.Z]` section all match the tag, failing fast instead of discovering a mismatch during a downstream publish step (as happened with `editors/vscode/package.json` on v0.20.0).
- feat(release): `publish-vscode.yml` gains an optional `ref` input, decoupled from `tag` — defaults to `tag`, but lets a fix-forward publish (e.g. a version-file correction on `main`) build from a different ref without moving/retagging the app release and re-triggering the whole cross-platform build matrix.
- feat(release): `publish-gate.yml` gains a `workflow_dispatch` trigger (`tag`, `run_id` inputs) so the msstore/vscode approval gate can be re-run manually for an already-built release, without waiting for a fresh `Release` `workflow_run` event.
- feat(vscode): register a `DocumentSymbolProvider` for Outline/breadcrumbs on native TOML/YAML text editors — the extension host loads the wasm core itself (raw-bytes init, no webview involved) and maps the read-only `ConfySession.outline()` tree onto hierarchical `DocumentSymbol`s, converting the core's UTF-8 byte offsets to VS Code's UTF-16 positions (`byteToPosition.ts`, plain-`node` unit test). Scoped to VS Code's native editor only (the custom editor tab stays as-is); malformed/mid-edit documents degrade to an empty Outline instead of erroring. Adds the extension's first explicit `"activationEvents": ["onStartupFinished"]` (a runtime-only provider registration has no declarative activation equivalent).
- feat(core+tui+web+touch): schema-warning discoverability — two additive facets, both driven by `Session::revalidate_schema`'s new `SchemaState.warning_ancestors` ancestor-path set: (1) a branch containing at least one schema violation anywhere in its subtree now shows a lightweight marker (TUI: `⚠` glyph after the `●` selection marker; web/touch: an amber corner dot in the existing `.schema-violation` color vocabulary, distinct from the own-row cue), computed per-row via `ViewRow.has_descendant_warning` and shown regardless of the branch's own expand state — a stable "there's a warning under here" cue that doesn't disappear the moment the branch is opened, per manual-test feedback; (2) the `f` type-filter popup gains an independent "Flags"/`(!) has warning` facet (`Cell::Warning`, `TypeFilter.warning_only`), ANDed with the existing Type/Sign facets and composing with `Reverse` — web/touch inherit the new facet automatically via the shared `layout()`/`Cell` rendering, no host-specific UI work needed.
- fix(tui,web,touch): unified the "there's a schema warning here" leading indicator across all three surfaces and to every warning-carrying row, not just the branch-summary case. TUI: replaced the branch-only `⚠` (variable emoji-presentation width across terminals) with a single-width `▲` glyph, now shown on any row with its own violation too (previously own-row violations only got the yellow text, no leading glyph). Desktop/touch: the own-row `.schema-violation` cue now also gets the same left-edge, vertically-centered amber dot the branch `.warn-branch` cue already used (touch previously only had a dashed outline for its own-row case, no dot); both platforms' duplicate `::after` rules consolidated into one shared selector, nudged to `left:7px` (was `2px`) so it no longer collides with `.row.selected`'s left-edge selection bar.

### Fixed
- docs(vscode): `VSCODE.md` § Title-bar tab swap and `extension.ts`'s `swapEditorKind()` comment claimed closing the old tab while another view still held the shared `TextDocument` skipped VS Code's unsaved-changes prompt. Verified against `@types/vscode`'s own `TabGroups.close()` contract (no such carve-out — a dirty tab always confirms) and live-reproduced on VS Code 1.134: the title-bar buttons do prompt on a dirty document, unlike the native `breadcrumbs.showEditorType` dropdown (1.132+), which uses an internal editor-replace API extensions can't reach. Corrected both comments to state this as a known API limitation, and noted `breadcrumbs.showEditorType` as a prompt-free alternative confy already supports for free via its existing `contributes.customEditors` registration. No behavior change.
- fix(vscode): local `$schema`-referenced files (e.g. `$schema: "./x.schema.json"`) never resolved inside the VS Code extension host — `readSiblingFile` (`web/fs.ts`) only had a Tauri branch and unconditionally threw for every other host, and the VS Code webview protocol never carried a file path or read channel at all (the JSON-schema-support spec's rollout phasing never named VS Code as a target surface). Added a `read-schema-file`/`schema-file`/`schema-file-error` message pair (`web/vscode-protocol.ts`) so the extension host resolves the relative path against its own `document.uri` and reads it via `vscode.workspace.fs` (parity with the Tauri host: no `../` traversal sandboxing), replied over `web/vscode.ts`'s `requestSchemaFile` and consumed by a new VS Code branch in `readSiblingFile` (`web/fs-vscode-schema.spec.mjs`).
- fix(vscode): remote `$schema: "https://…"` URLs never resolved inside the VS Code extension host either — `fetchUrlFile` (`web/fs.ts`) called `fetch()` directly from the webview, blocked by its `connect-src ${webview.cspSource}` CSP (no external origins allowed). Added a `read-schema-url`/`schema-url`/`schema-url-error` message pair (`web/vscode-protocol.ts`) so the extension host — unsandboxed Node network access, no CSP — fetches the URL itself via the global `fetch` (Node 18, the extension's `esbuild` target) and replies with the text or an `HTTP {status} {statusText}` error, matching the existing browser/Tauri hosts' error format exactly. `resolveSchemaFetchRequest` (`web/host-io.ts`) now branches to `web/vscode.ts`'s new `requestSchemaUrl` when `isVsCode()`, otherwise unchanged. No timeout, content-type, or redirect validation added (parity with the pre-existing `fetchUrlFile` behavior); no CSP relaxation (`web/vscode-schema-url.spec.mjs`).
- fix(release): `publish-msstore.yml`'s `Submit and publish to the Store` step reliably failed mid-upload ("Uploading Bundle to Azure blob: N%" then "Error while uploading the application package." / nonzero exit) — msstore-cli v0.4.0 (the `microsoft/microsoft-store-apppublisher@v1.2` default) has an unhandled `ObjectDisposedException` race in its Azure blob upload progress callback that fail-fasts the NativeAOT process, confirmed and reproduced upstream (microsoft/msstore-cli#154), fix not yet released. Pinned the action's `version` input to `v0.3.9`, the last known-good release, until upstream ships the fix.
- fix(core): `project_entry_into`'s multi-segment (dotted-key) branch never widened its enclosing `[header]` table's `text_range` to cover a directly-owned dotted-key entry (e.g. `a.b = 1` under `[server]`), unlike the sibling single-segment branch which already did this correctly — the table's own span silently dropped its trailing dotted-key member. `widen_end` is now called from both branches; the synthetic ADR-0006 `Dotted` chain node created for the entry itself stays anchor-only, unaffected.
- fix(ffi): `functional_smoke.mjs`'s "Paste selects exactly the pasted node" check asserted the pre-`27f1b50` contract (bare core `Paste` re-selecting the pasted row) — `do_paste` deliberately clears selection instead (see `clipboard.rs`, fixed 2026-08-17 to stop a "copy into itself, rename, copy to root" failure chain); hosts that want a "paste selects" UX already issue their own follow-up `SetSelection` (`web/ui.ts`, `touch/app.ts`). Updated the smoke check to assert the actual current contract (selection cleared, cursor moves).

## [v0.20.0] - 2026-08-19

### Added
- feat(core): `PasteSlot` (`Into`/`After`) is now the shared target representation for every host — new `SessionSnapshot.paste_slot`, `Intent::SetPasteSlot`, `Session::pointer_slot(path, rel_y)` (pixel-position → target), `move_selection_to` gains `cut: bool` (ADR 0004 §1).
- feat(session): armed cut/copy mode is now a full cross-platform modal lock across TUI, Desktop, and Touch — all mutating operations (add, delete, rename, inline edit, remark, kind-switch, convert, undo, redo, reorder-grip drag, swipe-to-delete) and modal entries (search/filter, type filter, detail popup, language picker) are disabled while `clipboard.is_some()`, leaving only navigation, `ToggleExpand` (Space/caret), paste commit (`v`/`p`), and escape (`Esc`) active; attempting a disabled affordance surfaces a transient localized status/toast message (`core.clipboard.action-locked`) (ADR 0005 §5 / `ROW_STATE_MODEL.md` §5).
- feat(web): desktop now previews the armed paste target under the pointer before commit — while the clipboard is armed, moving the pointer over a candidate row live-classifies it via `session.pointerSlot(path, relY)` and paints the same `.drag-over-into`/`#dropLine` cue the committed target already uses, client-only (no `dispatch`, no re-render); falls back to redrawing the committed target when `pointerSlot` declines to classify the hovered row or the pointer leaves the tree, so the preview never shows a target a click there wouldn't actually commit (ADR 0005 §6a / `ROW_STATE_MODEL.md` §6a).
- feat(web): touch now previews the armed paste target during a body-drag, not just a stationary tap — while the clipboard is armed, dragging past the existing tap-vs-scroll dead zone continuously reclassifies the target row via `session.pointerSlot(path, relY)` and repaints the same `.drop-into`/`.reorder-line` cue the committed target already uses, client-only (no `dispatch`); release commits exactly one `SetPasteSlot`/`SetCursor` for wherever the drag ended (never a `Paste` — the FAB alone dispatches that). A pointerdown on the branch caret (`.caret`) now bails out of the drag-preview loop at the source, mirroring the existing `.drag-handle` reorder gate, so a stationary caret press still expands/collapses via `handleTap`'s unchanged `act === "caret"` branch (ADR 0005 §6b / `ROW_STATE_MODEL.md` §6b).
- feat(touch): armed-paste body-drag AND grip reorder-drag now share one edge auto-scroll — dragging near `.tree-pane`'s top/bottom edge scrolls the tree so an off-screen target can be reached, matching what desktop's native HTML5 drag-and-drop already gets for free from the browser during grip reorder (`web/dnd.ts`; no equivalent existed for touch's two custom pointer-driven drags). A single `requestAnimationFrame` loop (speed ramps up closer to the edge) re-runs whichever drag's own hit-test (`onPasteDragMove`/`onReorderMove`) against the same pointer position each tick, since content shifts under an otherwise-stationary finger; self-terminates once neither drag is active (`pasteDragActive`/`reordering` both go false on `pointerup`/`pointercancel`). Previously out of scope for the paste-drag half (ADR 0005 §6b) on the assumption a per-pointermove `dispatch` would fight `render()`'s scrollTop-restore latch — moot, since neither drag's hit-test dispatches mid-gesture; only release does.
- feat(web): desktop now re-selects the freshly pasted/moved node(s) after a successful `Paste` (direct `v`/menu, or a collision prompt resolved via `PromptKey`) — `send()` diffs `clipboard_count` across the dispatch and, once the paste has landed (`snap.mode` back to `Normal`, no error), reads the landing siblings via `session.children()` and issues one `SetSelection`. Client-side only, no core change: relies on desktop's existing `navSelect`/`focusRow` convention, which already collapses `Selection` onto the cursor on the very next plain nav/click, so the highlight never outlives the gesture that follows — unlike the real `Selection`-based auto-select `do_paste` shipped and then reverted (`e6f4965`/`27f1b50`), whose core-level state persisted through the TUI's cursor-only arrow keys with no such compensator. TUI/touch and `do_paste` are unchanged.
- feat(touch): touch now also re-selects the freshly pasted/moved node(s) after a successful `Paste`, mirroring desktop's `send()` compensator exactly — safe here for the same reason: touch's own `selectOnly()` already collapses `Selection` to a single path on every tap, so the extra highlight never outlives the tap that follows it. TUI is deliberately *not* given the same treatment: its arrow-key navigation never touches `Selection` at all (a locked TUI selection is meant to persist across nav until explicitly cleared), so porting the same compensator there would reintroduce the `e6f4965`/`27f1b50` stale-selection failure mode inside the TUI — see `docs/superpowers/audits/2026-08-19-clipboard-row-state-integration-audit.md` and `ROW_STATE_MODEL.md` §6d.

### Fixed
- fix(web): desktop's marquee (rubber-band) selection now bails immediately on an armed-clipboard mousedown, matching every other affordance ADR 0005 §5's modal lock already disables — previously, a click on a candidate paste target that incidentally moved more than the 4px drag tolerance (common mouse/trackpad jitter) would silently swallow the click (core's `SetSelection` already no-ops while armed, with no toast) and suppress the trailing native `click` `armedPasteTarget()` needs, leaving the paste target unset with zero user feedback. Found via a full clipboard-armed row-state consistency audit (`docs/superpowers/audits/2026-08-19-clipboard-row-state-integration-audit.md`).
- fix(core): multi-select drag/cut of array elements on TOML could fail with a "path not found" error whenever the selection included the array's highest-index element, e.g. selecting 2-3 items out of `arr = ["a", "b", "c"]` that included `arr[2]`. `move_nodes`'s deletion loop sorted sources only by path length, leaving same-length array-index siblings in whatever order the caller passed them; deleting a lower index first shifts the not-yet-deleted higher indices out from under their still-stale paths. JSON's and YAML's `move_nodes` already broke this tie by deleting same-array siblings highest-index-first — TOML's never got that fix. Ported it over (`crates/confy-core/src/model/cst_edit/move_paste.rs`).
- fix(tui): a locked-selection row no longer paints a grey background fill, and a **copy** source no longer paints the cursor's blue — the three full-row fills are now mutually exclusive and unambiguous (cursor blue, cut source green, copy source magenta), while locked selection is signalled solely by its `●` marker so it composes with any of them instead of hiding underneath a fill (ADR 0005 §2 / `ROW_STATE_MODEL.md` §3).
- fix(tui): the armed paste-target's green `Into` fill now suppresses the KIND column's own type colour the same way the cursor's blue and clip-source colours already do, closing a latent legibility gap (a colour-tagged type on the Into row could otherwise collide with its own fill) reachable only through the unvalidated WASM `Intent::SetPasteSlot` boundary (`ROW_STATE_MODEL.md` §9).
- fix(web): the desktop tree now paints the three row states as mutually exclusive full-row fills — cursor **and** hover share one blue fill (`--cursor-bg`), a cut source fills green (`--cut-bg`), a copy source fills purple (`--copy-bg`), replacing the old dashed-outline/opacity/strikethrough treatment — and locked selection is demoted from its fill+ring to a 3px leading `--sel-edge` bar (`::before`) so it composes with any fill instead of hiding underneath one; the dead `.row.cut` rule (no emitter) is gone (ADR 0005 §2 / `ROW_STATE_MODEL.md` §3).
- fix(touch): the touch tree now paints the same row-state fills as desktop — a clip-copy source fills purple (`--copy-bg`) and a clip-cut source green (`--cut-bg`), the resting (non-paste-mode) cursor row fills blue (`--cursor-bg`), and locked selection is demoted to a 3px leading `--sel-edge` bar so it composes with any fill — all now matching desktop's colors (ADR 0005 §2 / `ROW_STATE_MODEL.md` §3).
- fix(core): appending a new member to a `[T/D]` dotted table whose existing member's value contained 2+ levels of nested inline tables (`t.a = { b = { x = 1 } }`) panicked (`range end index … out of range`, `rowan::splice_children`) — `project_inline` also indexes an inline table's own members as `Target::Entry`, but their `SyntaxNode::index()` is relative to their immediate CST parent (the inline table), not the flat ROOT; `node_last_root_index` (`tree_nav.rs`) recursed past a member's own backing ROOT entry into those nested, container-relative indices and misread them as ROOT-child positions. Fixed by short-circuiting on `Target::Entry` exactly like its sibling `node_start_root_index` already does. Found and root-caused via `systematic-debugging` while grilling ADR 0004 (`docs/superpowers/audits/2026-08-16-clipboard-paste-bugs.md`).
- fix(core): a multi-node paste whose first fragment inserted successfully but whose second collided left `self.tree` (and therefore visible rows/cursor/selection) silently stale relative to the already-mutated document — `do_paste`'s NODE-PHASE grouped-insert loop never called `on_mutation_success` on its Collision/error early-returns, unlike the comment-phase loop a few lines below which already did. Fixed by re-borrowing `doc` per iteration and reprojecting on every early return.
- fix(core): pasting or drag-moving a node into a collapsed container left the cursor on the freshly-landed child without ever expanding the container, so the row stayed invisible and the very next cursor-relative action (rename via F2) silently no-opped — confirmed byte-for-byte on the real TUI binary. `do_paste` (`clipboard.rs`) now expands every collapsed ancestor of the destination (not just its immediate parent, mirroring `reveal_path`'s prefix-expansion — a deeply-nested target now surfaces correctly too), matching `add_node_impl`'s existing single-level idiom.
- fix(core): renaming a node (F2, or a TypeChange-confirmed dotted rename) updated `self.cursor` to the node's new path but never remapped `self.selection`, so a selection left behind by an earlier action (e.g. `do_paste` selecting its own freshly-pasted node) silently went stale — `selected_paths()` prefers a non-empty selection over the cursor, so the *next* copy captured a fragment from the stale, now-nonexistent pre-rename path instead of the renamed node. This is what actually produced the user-reported "copy a JSON table into itself, rename the nested copy, copy it to root" chain (`paste error: invalid fragment: fragment is not a value` then `delete error: path not found`) — the two bugs above compound: the expand-on-paste bug was necessary but not sufficient, confirmed by re-testing the full chain on the real TUI binary after fixing it alone and still reproducing both errors. New `Selection::remap_prefix` (`selection.rs`) rewrites every selected path (and the in-progress round's anchor) under a renamed prefix, called alongside the existing cursor remap at both rename call sites (`edit_commit`, `apply_deferred_rename`). With both fixes, the full reported chain now succeeds end-to-end on the first try.
- fix(core): `do_paste` left the freshly-pasted/moved node(s) in `self.selection` after every successful paste or drag-move. `self.selection` is a persistent, opt-in multi-select the user builds explicitly (`s` toggle / Shift-range) that deliberately survives plain cursor navigation — so a paste's auto-selection silently outlived the paste itself, and the very next unrelated cursor move + copy re-targeted the stale pasted/moved node instead of whatever the cursor now sat on ("after pasting and I move the selection and press c again... it copies the previous pasted node", user report). `do_paste` now clears `self.selection` on every paste/move and only moves the cursor onto the result — applies uniformly to keyboard paste and mouse drag-reorder for consistency, since the drag case carried the identical latent risk.
- fix(core): moving (cut→paste or drag-move) an `[[array-of-tables]]` entry into another `[A/T]` group now moves atomically — nested `[table]` sub-sections (`[fruit.physical]`) are reconstructed as nested sections under the destination entry instead of flattening to a dotted key (ADR 0004 §3, `aot_entry_section_body`); a **copy** (clipboard capture always pre-flattens via `aot_entry_member_fragments`) and a plain-array destination (array elements cannot carry `[table]` headers) still flatten to dotted fragments.
- fix(web): the schema-enum picker's type-change confirmation never actually appeared on Chromium-based browsers (Chrome/Edge/WebView2 desktop; fine on Firefox/Safari) — picking a mixed-type enum option (e.g. `schema-sample.json`'s demo `0` member) silently left the value unchanged instead of opening the "type string → integer?" prompt. Root cause: `focusSchemaEnumSelect` (`web/ui.ts`) rebinds a fresh `onchange`/`onblur` closure onto the picker's `<select>` on *every* render while still in `Mode::SchemaEnum` — including the reentrant render `SchemaEnumMove` triggers from inside the very `onchange` handler that's about to call `SchemaEnumCommit`. The old `settled` guard was a plain per-closure boolean, so the *newer* (never-fired) closure bound mid-flight still had `settled === false` when `SchemaEnumCommit`'s own re-render dropped the `<select>` from the DOM a moment later — and only Chromium fires `blur` synchronously on that removal (Firefox/Safari don't), so only Chromium hit the spurious `onblur` → `Escape`, cancelling the prompt before it could paint. Fixed by deferring the blur decision via `queueMicrotask` and checking the live session mode + whether a schema-enum select is still focused once every synchronous `send`/re-render in flight has settled, instead of a closure-local flag.
- fix(core): adding a node ("Add", `a`/`+`) to a completely empty (or comment/whitespace-only) YAML document failed with "path not found" on both the TUI and the web UI — confy's YAML parser only ever emits a top-level `MAPPING`/`SEQUENCE` when there's content to parse, so `find_container` (`crates/confy-core/src/model/yaml/edit/block.rs`) had nothing to find for a blank file's root `Insert`, even though appending the very first field is exactly what "Add" on an empty document should do. `insert()` now synthesizes a fresh root container from the fragment itself (mirroring the fragment's own shape — a `- ` prefix builds a sequence, else a mapping) when no container exists yet at the true document root; a missing *nested* parent still correctly reports `NotFound`.
- fix(web): entering copy/cut mode and then clicking a branch's expand caret always toggled the *clipboard source* node, never whatever branch was actually clicked — `focusRow`/`handleTap`'s armed-paste-mode branch (`web/ui.ts`, `web/touch/app.ts`) positions the paste target (`SetPasteSlot`) instead of moving the selection while a clipboard is armed (`Session::set_selection` is deliberately a no-op then, freezing the highlight on what's being pasted), but never moved the session cursor either — and the cursor-based `ToggleExpand` intent kept firing against wherever the cursor had been frozen since the Copy/Cut. Both caret handlers now send an explicit `SetCursor` to the clicked/tapped row before `ToggleExpand`, which is a same-path no-op outside paste mode (a plain click's own `SetSelection` already moves the cursor there) and doesn't disturb the just-armed paste slot (a separate session field).
- fix(tui): remarking a node (`r`) always snapped the cursor back to the first visible row instead of staying on the toggled row — `do_remark` called `on_mutation_success(None)`, and Remark changes a node's *addressing* (a keyed live node becomes a positional comment, or vice versa), so the stale `self.cursor` path no longer resolved post-mutation and `compute_rows`'s "snap to first row when the cursor's path vanished" fallback fired. `do_remark` now captures the cursor's visible *row index* before applying the mutation and re-anchors `self.cursor` to that same index afterward — the row's position among its siblings is unchanged by a kind swap, only its path scheme is.
- fix(core): toggling remark (`r`) on an implicit/mixed table — one with no `[table]` header of its own, defined only through a child section (e.g. `[profile.release]` present, no `[profile]`) — failed with `remark error: path not found`. `remark`'s path resolver only recognized a table via a direct `Target::Header`/`AotEntry` syntax element, which an implicit table has none of. It now falls back to fanning out over `table_member_spans` (the same mechanism `delete`/`move`/`replace` already use for implicit/mixed tables), commenting out each member span independently.
- fix(web): entering armed copy/cut mode on desktop or touch left the plain cursor's own row style (a solid blue/accent fill, `body.paste-mode .row.cursor`/`.app.paste-mode .row.cursor` in `web/style.css`/`web/touch/style.css`) active and reused as a second, independently-tracked "▸ paste here" indicator — competing with the real target cue (`.drag-over-into`/`#dropLine` on desktop, `.drop-into`/`.reorder-line` on touch), which is pointer/click-driven and the only one that should exist during paste mode (the TUI already gets this right: `active_slot.is_some()` unconditionally suppresses the blue cursor style, ADR 0005 §2). On desktop the plain `:hover` blue compounded this — moving the pointer to preview the target (`onArmedPasteHover`) painted a second blue fill on the hovered row right alongside the green preview cue. Fixed at the render layer, not by chasing cursor-sync: both `.row.cursor`'s and `.row:hover`'s blue background are now scoped to `body:not(.paste-mode)`/`.app:not(.paste-mode)`, so neither paints anything while armed and the clip-source color (`.clip-copy`/`.clip-cut`) or green target cue shows through unobstructed — `Session::set_paste_slot` still moves `self.cursor` onto the committed slot's row (mirroring the TUI's keyboard-driven `move_paste_slot`), which now serves only its functional purpose (auto-scroll-to-target).
- fix(touch): entering armed copy/cut mode left native browser scrolling and text selection active on the tree, competing with the paste-target body-drag gesture (`onPasteDragMove`) for the same vertical pointer movement and letting a long-press start selecting row text instead of dragging the target. `.app.paste-mode .tree-pane`/`.row-main` now set `touch-action:none` and `user-select:none` (+ `-webkit-touch-callout:none`) for the duration of the armed state — there is no auto-scroll-during-drag (ADR 0005 §6b), so the tree is expected to already be positioned before arming.
- fix(web): five `snap.clipboard_count > 0` checks in `ui.ts` (add/menu/kind-badge/edit guards, `onTreeContext`) compared the `number | undefined`-typed field directly instead of via the file's established `(snap.clipboard_count ?? 0) > 0` idiom, failing `tsc --noEmit` (`TS18048`) and breaking the Web CI / Release builds. Brought in line with every other call site; updated `armed-paste.spec.mjs`/`modal-lock.spec.mjs`'s source-matching regexes accordingly.
- fix(release): `Desktop x86_64-pc-windows-msvc`'s build failed `web` test suite checks (`installTreeGestures found in source` and 5 others in `touch-paste-drag.spec.mjs`) that never failed on `Web CI` (ubuntu-only) — Windows' git checkout defaults to `core.autocrlf=true`, silently rewriting the repo's LF source to CRLF, which broke those specs' `\n`-anchored regexes matching against `touch/app.ts`. Added `.gitattributes` (`* text=auto eol=lf`) to force LF checkout on every platform.
- fix(release): `Build x86_64-unknown-linux-musl`/`aarch64-unknown-linux-gnu`'s `apt-get install` steps could hang 10+ minutes on a GH-hosted runner's background apt/dpkg lock instead of failing outright, stalling the whole Release matrix with no useful error. Both steps now bound each `apt-get` call with `timeout` and retry up to 3 times (`timeout-minutes: 6` overall), so lock contention surfaces as a quick self-healing retry instead of an indefinite hang.
- fix(release): `editors/vscode/package.json` was missed during the version bump (still `0.19.1`), failing `publish-vscode.yml`'s tag/`package.json` version-match check (`tag v0.20.0 does not match package.json version 0.19.1`). Bumped to `0.20.0`.

### Removed
- feat(web)!: removed the manual "attach schema" affordance — desktop's `{}` toolbar button and touch's "Attach schema…" menu entry, along with `Intent::SetSchema` and its dispatch arm. Schema now loads exclusively from in-document annotations (`$schema` JSON root key, YAML `# yaml-language-server: $schema=` modeline, TOML `#:schema` leading comment) via the existing `Session::detect_and_request_schema`, which every schema-driven feature (constrained-value picker, hover hint, violation status) already ran through regardless of how the schema was attached. The TUI's `--schema` CLI flag is unaffected — it never went through `Intent::SetSchema`, calling `apply_schema_text` directly at startup.

### Changed
- feat(core): moving or pasting a bare scalar out of a keyed array into a table/object/mapping now synthesizes `<arrayKey>_<index>` as its key (e.g. `nums = [10, 20, 30]`, moving index `1` out → `nums_1 = 20`) instead of the generic `placeholder` — across all three formats (TOML, JSON, YAML block/flow), for both drag/cut-move and copy+paste. Falls back to `placeholder` (still auto-renaming on collision, never prompting) when the array itself has no key to derive from (a nested/unkeyed array, or a root-level bare array). Object/table array elements are unaffected — they already unpack into their own member keys (TOML) or nest under the synthesized key with members intact (JSON, pre-existing per-format behavior).

### Docs
- docs(adr): add ADR 0004 — unify node copy/cut/paste/move targeting (`PasteSlot`) across TUI, web keyboard, web mouse, and touch; narrows the AoT-entry atomic-move fix to AoT/array destinations only, defers node-kind/format mechanics to `CONTEXT.md`/`BEHAVIOR_MATRIX.md` as the maintained source
- docs(adr): correct ADR 0004's Consequences bug list — the three "found while grilling, out of scope" bugs it cited are now fixed (with corrected root-cause descriptions matching the audit doc, which had drifted during debugging); drops the never-reproduced "unconfirmed YAML add-entry failure" claim as unsubstantiated; adds a note that `do_paste`'s no-longer-auto-selects fix is a constraint the ADR's future `pointer_slot`/`SetPasteSlot` work must preserve

- docs(adr): add ADR 0005 — formalize the row cursor/selection/clipboard-source state model (five layered states: Cursor, Focal row, Locked selection, Clipboard-armed, Clipboard source) and unify its visual language and keybindings across TUI/desktop/touch; new `ROW_STATE_MODEL.md` holds the per-platform binding tables, visual spec, keybinding table, cut/copy-mode modal-lock spec, the desktop-hover-preview and touch-drag-to-target target-positioning designs, and the phased implementation task list. Corrects `CONTEXT.md`'s Locked selection glossary (previously assumed TUI-only) and fixes `WEBUI.md`'s "Paste mode" paragraph, which still described the pre-ADR-0004 `SetCursor` behavior instead of the shipped `SetPasteSlot`/`pointerSlot()` flow.
- docs(adr): mark ADR 0005 `implemented` — all five phases (visual language, keybinding reversal, cut/copy modal lock, desktop hover preview, touch drag-to-target) shipped, reviewed, and merged to `main`; `ROW_STATE_MODEL.md` §8's checklist is fully ticked.

## [v0.19.1] - 2026-08-12

### Fixed
- fix(web): schema-enum `<select>`'s dropdown arrow was clipped when its row's value cell was flex-compressed (long trailing comment, narrow window) — `.val`'s ellipsis-truncation sizing (`overflow:hidden; min-width:0`, needed to clamp long static text) also clamped a live editing control to less than its own rendered width. `render.ts` now tags the `.val` cell with an `editing` class while it holds the schema-enum `<select>` or the plain inline-edit `<input>`, and that class resets to `overflow:visible; min-width:max-content` so the control is never narrower than its content.
- fix(web): schema-enum picker relocated onto whatever row was clicked next, covering that row's real value — clicking a different row while the picker was open moved the tree cursor (`SetSelection`/`SetCursor`, unguarded by `Mode::SchemaEnum`) without cancelling the picker, and `renderValue` draws the picker on whichever row is `is_cursor`, so it visually "followed" the click onto an unrelated row (the eventual commit still silently applied to the *original* field). `focusSchemaEnumSelect` (`web/ui.ts`) now cancels the picker on blur — mirroring Escape, and matching the plain inline-edit `<input>`'s existing commit-on-blur behavior — guarded by a `settled` flag (not `document.contains`, which reads `true` mid-blur during the picker's own commit-triggered re-render) so a real option pick still commits normally.
- fix(web): `npm test` was bash-only syntax (`for f in *.spec.mjs; do node "$f" || exit 1; done`), breaking on Windows CI (`cmd.exe`, not bash) — the Windows desktop release leg failed immediately with `"f was unexpected at this time."` before running a single spec, never caught since this test script had never run on Windows CI until this release. New `web/run-tests.mjs` is a small cross-platform runner (`spawnSync(process.execPath, ...)`, no shell syntax in the npm script itself) — same sorts-and-runs-each-until-first-failure behavior as the old loop.
- fix(web): `cf-build.sh` typechecked before `web/pkg` existed on a clean checkout — `npm run typecheck && npm test && node build.mjs` ran in that order, but `node build.mjs` is what copies `crates/confy-ffi/pkg` → `web/pkg` (which `confy.ts` imports); locally masked by a leftover `web/pkg/` from a prior build, a genuinely clean CI checkout failed `tsc` with `TS2307`. Reordered to build (wasm-pack + esbuild bundle) before typecheck/test.

## [v0.19.0] - 2026-08-12

### Added
- feat(core+tui+web+touch+tauri): JSON Schema support — in-file hint detection ($schema key, yaml-language-server modeline, TOML `#:schema` comment) with explicit override, `jsonschema`-crate-backed validation surfaced as soft (never-blocking) per-row warnings, and constrained enum/const inline editing (TUI popup, web `<select>`, touch bottom sheet) across TOML/JSON/YAML on every surface (TUI, web desktop, touch, Tauri desktop/mobile)
- feat(web+touch): JSON Schema editing UX refinements — the "Edit" ctx-menu item (web) and a new "Edit in editor" detail-panel button (touch, also on desktop's side panel) now always force the free-form popup editor, bypassing the schema-enum picker (`Intent::BeginEditExternal`); the shared detail panel's value field now also triggers the schema-select when enum-constrained, giving touch full parity with the tree (touch's only value-edit surface); a schema-violating commit now surfaces an advisory status message combining the violation text with a "valid values: …"/"must be between X and Y" suggestion (soft — the commit still succeeds); a desktop-only hover tooltip on schema-constrained rows shows the same suggestion via a native `title` attribute, lazily resolved per-hover
- feat(tui,web): 2 more JSON Schema editing UX refinements — the web/touch detail panel's "Edit in editor" button is now labeled "Editor" (i18n string only); the TUI's schema constraint hint ("Valid values: …" / "Must be between X and Y, a multiple of Z") now appears and clears dynamically on the status line as the cursor moves onto and off a schema-constrained node — a tooltip-like effect with no mouse involved (`EditHint::describe()`, mirrors web's existing hover-tooltip wording), yielding to any explicit status message (e.g. a just-committed violation) which still takes priority
- feat(web,touch): dynamic idle schema hint on the status line, matching the TUI — the status line now surfaces the current cursor/selected node's schema constraint ("Valid values: …" / "Must be between X and Y, a multiple of Z") whenever nothing more important is showing, clearing the instant selection moves off a constrained node; touch (no hover) gets this feedback for the first time, and desktop's existing hover-only tooltip now shares the same formatter (`schemaHintText`, `panel.ts`) — all three surfaces (TUI, web desktop, touch) behave identically
- feat(vscode): theme menu — "…" → confy: Theme (Auto / Light / Dark), replacing the previously implicit auto-follow-VS-Code behavior; persisted via globalState, same pattern as the existing Language submenu
- feat(ci): selective publish gate — `publish-gate.yml` splits its single approval job into one job per store (`publish-gate-msstore`, `publish-gate-vscode` environments), so a release's Microsoft Store and VS Code Marketplace/Open VSX submissions can be approved independently in the same "Review pending deployments" screen instead of all-or-nothing; replaces the shared `publish-gate` environment (removed)
- feat(ci): RELEASES.md version auto-sync — `scripts/sync-releases-md.sh` patches the "Current version" column and pushes to `main` from `release.yml`/`publish-msstore.yml`/`publish-vscode.yml` right after each channel actually goes live (retries on push race), replacing the manual, previously-forgotten update step
- docs(readme): document `wenget add confy` as the recommended cross-platform CLI install method (confy is registered in the `wenget` bucket manifest)
- feat(android): Google Play prep — conditional release `signingConfig` in `gen/android/app/build.gradle.kts` (gitignored `keystore.properties`, CI-secret-driven, falls back to unsigned when absent), tag-derived `versionCode` override (`CONFY_VERSION_CODE` env var, avoids Tauri's stateless-CI-unsafe `autoIncrementVersionCode`), new `/privacy` static route (`web/privacy.html`, mirrors `PRIVACY.md`) as the canonical privacy-policy URL for every store listing, draft Play feature graphic (`crates/confy-tauri/play/`); RELEASES.md gains an in-development "Android Google Play" row — no Play Console account yet, `publish-play.yml` CI not built
- feat(android): Save As / Convert-to-new-file enabled on Android (M2) — a new `create_writable` command in `tauri-plugin-confy-picker` (`ACTION_CREATE_DOCUMENT` + `takePersistableUriPermission`, mirroring the existing `pick_writable`/open flow) replaces stock `tauri-plugin-dialog`'s Android `saveFileDialog`, which never took a persistable write grant (confirmed by reading its source; see `docs/adr/0001-android-save-as-persistable-grant.md`); `canSaveAs()` now returns `true` on every platform (was hardcoded `false` on Tauri mobile since M1). Verified end-to-end on real hardware, including the hard acceptance bar: a file created via Save As can be reopened and re-saved after a full app kill + relaunch (persistable grant survives process death).

- feat(core+tui+web): PageUp/PageDown/Home/End on the schema-enum picker — Home/End jump to the first/last option, PageUp/PageDown jump a page (clamped, not wrapping, via new `Session::schema_enum_jump`/`Intent::SchemaEnumJump`); the TUI's popup now scrolls to keep the cursor on-screen for lists taller than the popup. Surfaced and fixed a real desktop-web bug along the way: the picker `<select>` lives inside the tree, so Arrow/Home/End/Enter keydowns were bubbling into the tree's own global shortcut handler and getting reinterpreted as tree-row navigation instead of reaching the picker (Up/Down silently moved the *tree* cursor, Enter toggled branch expand) — fixed with a dedicated `SchemaEnum` mode block in `onKey`, mirroring the existing `KindSwitch`/`TypeFilter` mode blocks
- test(web): CI enforcement for `web/` — new `.github/workflows/web-ci.yml` (push/PR on `web/**` + `crates/confy-ffi/**`, visibility only, not a merge gate — `confy` is a single-committer direct-push-to-`main` repo, so a required status check would reject the maintainer's own pushes) plus `web/cf-build.sh` now runs `npm ci && npm run typecheck && npm test` before assembling `dist/`, so a type error or test regression fails the release/CF Pages build too, not just PR time (`docs/superpowers/plans/2026-08-11-web-code-audit-remediation-plan.md`)
- test(web): new `render.spec.mjs`/`host-io.spec.mjs`/`key-intent.spec.mjs` — HTML-escaping regression coverage for `render.ts`/`panel.ts` (hostile `<script>`/`&`/`"` payloads in key/value/comment fields), save/open/convert flow coverage for `host-io.ts` (`doQuickSave`/`doSaveAsCopy`/`doConvertWrite`/`openFromUrl` against a fake `HostIo`+`FsHandle`, no wasm/DOM), and 44 table-driven cases covering the keyboard dispatcher's full mode-precedence chain (Edit > Prompt > Convert > TypeFilter > KindSwitch > SchemaEnum > Help > tree shortcuts); `package.json`'s `test` script now runs every `*.spec.mjs` file instead of just `toolbar-fold.spec.mjs`
- feat(tui): `confy <url>` — open a config directly from an `http(s)://` URL. Before launching the TUI, the CLI prompts on the terminal for a local save path (suggesting a name derived from the URL's last path segment; accepting the blank default keeps the suggestion), fetches the URL, and writes it there — the normal load path then opens it exactly like any pre-existing file (`source_path` is always set; no read-only mode, no new TUI `Mode`). A non-interactive stdin aborts before any network call, mirroring the existing `create_missing_file` non-TTY guard.

### Fixed
- fix(android): `keystore.properties`' `keyPassword` didn't match `confy-release.keystore` — PKCS12 keystores use a single password for both the store and the key (`keytool -keypasswd` errors "not supported" on PKCS12, confirming there's no independent key password), so the previously-recorded distinct value was never validated against the real keystore. Release builds failed with `KeytoolException: ... Given final block not properly padded`. Fixed locally (gitignored `keystore.properties` + Keychain custody record now both use the store password for `keyPassword`); this unblocked the first-ever end-to-end release build/sign/install verification (see below).
- fix(docs): `crates/confy-tauri/msix/STORE.md` — stale single shared `publish-gate` environment description corrected to match the `1c3e01a` per-store `publish-gate-msstore`/`publish-gate-vscode` split; added the (manual, non-automatable) Partner Center privacy-policy-URL setup step
- fix(android): "Open with"/share chooser visibility (M2) — confy didn't reliably appear when opening/sharing `.toml`/`.json`/`.yaml` from a file manager. Root cause: the auto-generated `AndroidManifest.xml` intent-filters declare `android:mimeType` with no `android:scheme`, so Android's path-pattern matching (which requires a scheme) never activated, leaving matches dependent on each file manager's own inconsistent MIME-type guess (already noted as "mitigated, not fully fixable" in the M1 plan). Fixed with hand-authored `<intent-filter>` blocks (VIEW + SEND + SEND_MULTIPLE, `scheme=content`/`file`, `host=*`, wildcard `mimeType="*/*"` constrained by a real `pathPattern`) placed outside the `tauri-file-associations` auto-generated markers in `AndroidManifest.xml` so they survive every `cargo tauri android build` regeneration; see `docs/superpowers/plans/2026-08-06-mobile-m2-saveas-fileassoc-plan.md`. Verified on-device (MaterialFiles) for all three formats, a negative check (an unrelated `.jpg` does not match), and the existing M1 cold-start-open path is unaffected.
- fix(web): clicking a row's value/key/kind-badge/caret/± only moved the cursor, leaving the visible `.selected` highlight (and the copy/cut/delete target) on whatever was previously selected — only a click on the row's blank body area actually selected it. `onTreeClick`'s caret/kind-badge/±/key/value/trailing-comment branches (`web/ui.ts`) and the touch UI's caret tap (`web/touch/app.ts`) now route through the same selection-resolution gesture (plain replaces, ⇧ ranges, ⌘/Ctrl toggles) as a body click, so any click on a row selects it; paste mode still freezes selection to a bare cursor move, unchanged.
- fix(core): schema-enum picker commits bypassed the type-change confirmation — `Session::schema_enum_commit` applied its `Mutation::Replace` directly and unconditionally, skipping the `Mode::Prompt(TypeChange)` confirmation gate every other value commit already has; a schema enum/const mixing types (e.g. a string and a numeric const) could silently change a node's underlying type on any surface (TUI, web, touch — core-shared logic, not per-host). Fixed by routing `schema_enum_commit` through `edit_commit` itself (same synthetic-`Mode::Edit` trick the Web one-shot `commit_edit` already uses), so it inherits the type-check, confirmation prompt, and trailing-comment preservation for free
- fix(web): built-in demo sample had no reachable repro for the type-change confirmation above — `schema.editor`'s enum was all-strings, so no member could ever trigger one; added a mixed-type `0` member (`schema-sample.json`) so the confirmation is demoable straight from the shipped sample, no custom schema needed
- fix(web): removed two unconditional `console.log` debug calls in `touch/app.ts`'s `openOpenedUrl` — left over from now-resolved `content://` Android read/dedupe debugging (`CHANGELOG.md`'s own M1/M2 entries show the underlying bug shipped fixed); no open issue tracked it and no active reason remained to keep them

### Verified
- android: full local build/sign/install pipeline run end-to-end for the first time (`realme:5555`, Android 12, real hardware) — `cargo tauri android build --debug --apk` and `--apk` (release) both succeed from a clean tree without Android Studio (CLI-only `android-commandlinetools` toolchain); `apksigner verify` confirms the release APK is signed with the `confy-release` cert (debug stays Android-Debug-signed); merged manifest carries `usesCleartextTraffic=false` in release vs `true` in debug as configured; minify+proguard shrinks the universal release APK 585MB→25.6MB; both variants install, launch without a crash (`logcat` clean of `FATAL`/`AndroidRuntime`), and render/respond to touch (tap-to-select verified via on-device screenshot, confirming the wasm↔JS bridge survives release minification). Regression gate (`cargo build/clippy -D warnings/fmt --check`, `web/` `tsc --noEmit`) clean; `functional_smoke.mjs` has one pre-existing unrelated failure (`grid active after toggle`, TypeFilter grid — present on `main` before this session, not touched here).

### Refactor
- refactor(core): `Session::dispatch(Intent)` split into `apply(Intent) -> ApplyOutcome` (mutation only — cursor/kind-switch/filter/etc. routing, no row rebuild or render snapshot) plus a thin `dispatch()` wrapper that calls `apply()` then `compute_rows()`/`snapshot()` and overlays `ApplyOutcome`'s transient signals (`convert_write`, `quit`, `schema_fetch_request`). Zero behavior change for existing `dispatch()` callers (Web/wasm/Tauri/VS Code) — same signature, same output, same test coverage (472 lib tests unchanged, 2 new `session_headless` tests proving `apply()` skips the cursor-snap `dispatch()` performs). Prerequisite for a future TUI → `dispatch(Intent)` routing pass (`docs/adr/0003-*.md`, Task 13 of the 2026-08-11 audit remediation, descoped): routing TUI's pure-navigation intents (`CursorDown`/`Up`/`Home`/`End`/`PageUp`/`PageDown`) through the old single-shape `dispatch()` would have paid its unconditional O(visible-node) `compute_rows()` + full-snapshot cost on every arrow-key press — reintroducing, at the input layer, the exact "rebuild everything every frame" cost Task 16 (see `[Unreleased]` above the last release) had just eliminated at the render layer. `apply()` gives a future TUI conversion a cheap path for those intents while still centralizing the cross-cutting logic (shift-select reset, `ToggleExpand` branch/leaf decision) the audit originally flagged as hand-duplicated.
- refactor(tui): TUI routes through `Session::apply(Intent)` — `app.rs`'s ~65 wrapper methods with an exact-match `Intent` variant (navigation, filter, type-filter, kind-switch, convert, detail, help, selection, inline-edit, mutations, undo/redo, escape, prompt) now call `self.session.apply(Intent::_)` internally instead of the raw `Session` method, same signature/behavior (`cargo test -p confy-tui`'s 178 tests pass unchanged). Also fixes the audit's actual named finding — two real hand-duplicated cross-cutting decisions, not just raw calls: `mod.rs`'s `ToggleExpand` handler had its own `is_branch` check duplicating `apply()`'s branch-toggles/leaf-opens-detail decision (now `apply(Intent::ToggleExpand)` decides; `mod.rs` keeps only a cheap `is_branch` read to skip the row rebuild when a leaf opened Detail instead), and `Quit`'s `if confirm_quit() {} else if quit_requested() {}` gate duplicated `Intent::QuitRequested`'s own gate (now one `apply()` call, `ApplyOutcome::quit` replaces both). Left un-routed, each for a real semantic reason (see `docs/adr/0003-*.md` Resolution section): `toggle_expand()` itself (paste mode needs the raw unconditional toggle), `convert_pick_format`/`edit_clamp_scroll` (their `Intent` variants are deliberately host-divergent — fs-free `None` stem, Web-only no-op scroll clamp), and `apply_replace`/`begin_inline_edit`/`edit_node`/`save`/`lang_picker_commit` (host-specific fs/`$EDITOR` I/O or smart routing already decided one layer up, no fs-free `Intent` equivalent). Verified live: real TUI binary smoke-tested (leaf/branch `ToggleExpand`, clean/dirty `Quit` confirm flow) in addition to `cargo test --workspace` and `cargo clippy --workspace --all-targets -D warnings`, both clean. Closes Task 13 of the 2026-08-11 audit remediation (16/16).
- refactor(web): deduped `modeTag`/`batch()` — `web/ui.ts` and `web/touch/app.ts` each carried a byte-identical `modeTag` and a near-identical batching-flag/try-finally `batch()`; both now share `web/mode.ts` (`modeTag`, a `createBatcher(render, afterRender?)` factory each host instantiates with its own post-render hook)
- refactor(web): extracted `resolveKeyIntent` (new `web/key-intent.ts`) — `ui.ts`'s `onKey`, the primary keyboard-input dispatcher, is now a thin wrapper around a pure `(mode, key, mods, rawView, vshost) -> KeyResolution` function covering every mode-precedence branch, unit-tested without a DOM (`key-intent.spec.mjs`). `onKey` keeps every actual side effect — modal guards, `preventDefault`, and the handful of branches that are more than a single `Intent` dispatch (`navSelect`'s compound `SetSelection`, `toggleSelectedBranches`, `doSave`/`doOpen`, `runSaveConvertShared`, the DOM-derived TypeFilter page-size). No behavior change — verified via the new unit suite and live keyboard-driven smoke testing of every mode transition on both desktop and touch.

## [v0.18.1] - 2026-07-29

### Fixed
- fix(core): Reverse type-filter had no visible effect on Table/Array — `recompute_filter`'s ancestor-context rule (an ancestor of any match stays visible) resurrected an excluded container the instant one of its own children legitimately passed the reversed filter, which is virtually always true for non-empty containers; Scalar/Comment reversal looked fine only because leaves have no children to trigger this. Fixed by pruning the whole subtree under a node that's a deliberate Reverse-exclusion target (`TypeFilter::is_reverse_excluded`, new `base_match` helper) instead of just dropping it from the match set.

## [v0.18.0] - 2026-07-29

### Added
- feat(core+tui+web): `f` type-filter panel — Reverse toggle inverts the sign/type match (`Cell::Reverse`, first row of `layout()`; a no-op with nothing else selected, so it can't blank the tree before a facet is picked); Home/End jump to the first/last nav row, PageUp/PageDown jump by the popup's visible-height nav-row count (`ui::type_filter_page_step` in TUI, scroll-ratio in Web)
- feat(ci): automate Microsoft Store re-submission — new `msstore` job in `release.yml` runs `msstore reconfigure`/`msstore publish` against the Partner Center Submission API on every `v*.*.*` tag, gated behind a `msstore-publish` GitHub Environment approval; replaces the manual Partner Center upload documented in `STORE.md`

### Fixed
- fix(web): duplicate/misaligned clear "×" on the filter bar search box — Safari/Edge/Chromium (and every host embedding the shared web UI: Windows/macOS desktop apps, VS Code extension) render their own native `type="search"` cancel button alongside the custom `.clear` button; suppress it via `::-webkit-search-cancel-button{-webkit-appearance:none}` in both `web/style.css` and `web/touch/style.css`
- fix(msix): match Store-reserved DisplayName — Partner Center rejected the submission because `Package/Properties/DisplayName` "confy" wasn't reserved; renamed to the reserved "Confy — TOML/JSON/YAML Editor"

### Docs
- docs: add standalone PRIVACY.md for Microsoft Store submission — mirrors the existing in-app About-tab privacy statement (`state.rs` `ABOUT_TEXT`)
- docs: update RELEASES.md versions for v0.17.0 (app + VS Code extension)

## [v0.17.0] - 2026-07-22

### Added
- feat(web): freeze breadcrumb bar path while browsing segment mini-trees — active in all web hosts (browser / Tauri / VS Code webview)
- feat(vscode): publish confy for VS Code to the VS Marketplace + Open VSX — `publish-vscode.yml` builds/publishes on `vscode-v*.*.*` tags
- feat(tauri): hide toolbar header on desktop — native menu bar + window title (filename/format/dirty) replace it; File ▸ New/Open become format/source submenus

### Fixed
- fix(web): prevent duplicate file extensions in Firefox download fallback — `ensureExt` helper ensures `sample.json` saves as `sample.json`, not `sample.json.json`

### Docs
- docs: add RELEASES.md — distribution channel overview (per-platform method/trigger/version/status)
- docs(vscode): document publishing setup, fix stale marketplace links
- docs: refresh platform/version info across CLAUDE.md and editors/vscode/README (compress point-in-time history)

### Notes
- The VS Code extension is republished at `vscode-v0.17.0` (was `0.3.0`) to ship the breadcrumb fix and pick up the shared web bundle; its version line is now aligned with the app.

## [v0.16.0] - 2026-07-17

### Added
- feat(core): RevealPath intent — expand ancestors + set cursor + select; filter-hidden targets keep cursor
- feat(core+ffi): children_of query for breadcrumb mini-tree
- feat(core): expose undo-history depth as SessionSnapshot.history_len
- feat(core): Privacy Policy paragraph in shared About text
- feat(web): breadcrumb bar + mini-tree picker with direct-jump and center-scroll Reveal
- feat(web): VS Code webview host protocol + adapter modules
- feat(vscode): M1.5 — CustomTextEditorProvider rebase, shared TextDocument owns dirty/undo/save
- feat(vscode): 0.2.1 — in-place tab swap + "Open Text Editor to the Side" button
- feat(vscode): M1.6 (0.3.0) — Save As/Convert, header hidden, Help/About/Language in "…" menu
- feat(vscode): Marketplace icon

### Fixed
- fix(vscode): Save As/Convert shortcut claimed by workbench — moved to extension keybinding
- fix(vscode): move "Open Text Editor to the Side" to editor title bar
- fix(tui+web): Help/About overlay wraps long lines

### Docs
- docs: VS Code M1 plan/spec, TAURI.md/VSCODE.md split, CLAUDE.md breadcrumb docs

### Detailed history — 2026-07-15 to 2026-07-17 (pre-consolidation entries for v0.16.0)
- 2026-07-15 feat(core): expose undo-history depth as SessionSnapshot.history_len (VS Code host edit-stack mirror)
- 2026-07-15 feat(web): add VS Code webview host protocol + adapter modules
- 2026-07-15 feat(web): VS Code webview host wiring in ui.ts (boot, save/undo/convert reroutes, chrome trim)
- 2026-07-15 feat(vscode): extension scaffold — custom editor boots the confy webview
- 2026-07-15 feat(vscode): document lifecycle — dirty tracking, save with save-ok ack, undo/redo single owner, revert, hot-exit backup
- 2026-07-15 feat(vscode): raw preview command, convert-save dialog, parse-error fallback
- 2026-07-15 feat(vscode): package sideload .vsix + docs (M1)
- 2026-07-16 feat(vscode): editor title-bar toggle — Open with confy ⇄ Reopen as Text Editor (in-place tab swap; dirty text buffer saved before switching); shared-dirty-state sync via CustomTextEditorProvider recorded as the M1.5 goal
- 2026-07-16 feat(vscode): M1.5 — rebase onto CustomTextEditorProvider: shared TextDocument owns dirty/undo/save/hot-exit; toggle carries unsaved changes; editable side-by-side text sync (150ms debounce, tree pauses on invalid text); raw preview retired for "Open Text Editor to the Side"; vsix 0.2.0
- 2026-07-17 feat(vscode): 0.2.1 — title-bar toggle now truly swaps in place (open new view, then close the old tab, avoiding VS Code's per-(uri,viewType) tab stacking); "Open Text Editor to the Side" promoted to an editor/title icon button next to "Reopen as Text Editor" (was command-palette only)
- 2026-07-17 feat(vscode): M1.6 (0.3.0) — Save As/Convert entry point: new `exec`/`set-lang` protocol messages, `confy.saveAsConvert` editor-title command + ⇧⌘S/Ctrl-Shift-S opening confy's own Save/Convert dialog; whole confy toolbar header hidden in this host (`header.toolbar` under `body.host-vscode`, was three buttons) with Save As/Convert, Help, About, and a native Language submenu (English/繁體中文 picked directly, no QuickPick) moved to the editor title's "…" More Actions menu (`confy.help`/`confy.about`/`confy.langEnglish`/`confy.langZhTw` + `contributes.submenus`, routed via `ConfyEditorProvider.postToActive` tracking the active `WebviewPanel`); language choice persists in `context.globalState["confy.lang"]` and overrides `vscode.env.language` on the next boot; VS Code-specific Help text variant (`help-content.ts`'s `HELP_TEXT_VSCODE`/zh-TW pair) drops the inapplicable Ctrl-o/q lines and documents ⇧⌘S + the "…" menu
- 2026-07-17 fix(vscode): ⇧⌘S/Ctrl-Shift-S Save As/Convert — the workbench's own Save-As keybinding was claiming the keystroke before the webview's `keydown` handler ever saw it (confirmed by manual testing), so the shortcut is now an extension-side `contributes.keybindings` rebind of `confy.saveAsConvert` (scoped to `activeCustomEditorId == 'confy.editor'`) instead of a webview-side intercept, which is removed as dead code
- 2026-07-17 feat(vscode): Marketplace icon — `editors/vscode/icon.png` (the confy brand icon) wired via `package.json`'s `icon` field, verified with a local `vsce package`
- 2026-07-17 feat(core): Privacy Policy paragraph added to the shared About text (`ABOUT_TEXT`/`ABOUT_TEXT_ZH_TW`) — one core string consumed verbatim by TUI, Web, Touch, Tauri, and VS Code, so all hosts pick it up with no per-host changes
- 2026-07-17 fix(tui+web): Help/About overlay line wrapping — the TUI overlay now wraps long lines (`Paragraph::wrap`) with scroll math based on a `wrapped_line_count` matching the popup's 65% width; the web VS Code help paragraphs are joined into single lines so CSS wraps them naturally

### Unreleased Update — 2026-07-17
- feat(web): breadcrumb bar + mini-tree picker below the filter row — segment
  click opens a lazy mini document tree (new ffi `children(path)` query),
  row click Reveals the node via the new core `RevealPath` intent (expands
  ancestors + sets cursor; filter-hidden targets keep the cursor and report on
  the status line). All web hosts (browser / Tauri / VS Code webview); touch UI
  excluded. New glossary term: Reveal (CONTEXT.md §Operations).
- feat(web+core): breadcrumb direct-jump refinement — clicking a segment now
  Reveals it directly (no mini-tree detour); the mini-tree moves to the `›`
  separators (plus a trailing `›` after the current node, the only entry when
  the cursor is on the root). `RevealPath` additionally selects the revealed
  node (single-node selection; skipped for the root and in paste mode where the
  clipboard freezes selection), and the web UI smooth-scrolls the revealed row
  to the viewport center (clamped at the top/bottom edges).
- fix(web): the Save As/Convert chevron now toggles — a second click while its
  menu is open closes it (same pattern as the language and ⋯ buttons).

## [v0.15.0] - 2026-07-15

### Added
- **feat(mobile): Android toolchain + write-in-place decision gate passed (M1 Task 0)**
  (2026-07-13). `crates/confy-tauri` restructured into a `[lib] confy_tauri_lib` (mobile entry
  point) + thin `main.rs`, unblocking `cargo tauri android init`/`build`. Spiked whether a
  picked `content://` URI survives a full app restart for write-back — it does, but **only**
  with a new workspace crate, `crates/tauri-plugin-confy-picker`
  (`ACTION_OPEN_DOCUMENT` + `takePersistableUriPermission`), because stock
  `tauri-plugin-dialog`'s Android picker uses `ACTION_GET_CONTENT`, which never grants write
  access at all (confirmed against the plugin's own source; unresolved upstream as of
  `tauri-plugin-dialog` 2.7.1). Gate passes on real hardware: pick → write → kill app →
  relaunch → write again (no re-pick) → read back both markers. `bundle.fileAssociations`
  needs no manual `AndroidManifest.xml` edit — Tauri's build system generates the Android
  intent-filter automatically. Full findings recorded in
  `docs/superpowers/plans/2026-07-13-mobile-m1-android-plan.md` (Task 0 outcome).

### Fixed
- **fix(desktop): Windows build failure — `RunEvent::Opened` is macOS/iOS/Android-only**
  (2026-07-15). `crates/confy-tauri/src/lib.rs` used `RunEvent::Opened` unconditionally in
  `run()`'s event handler, but that variant doesn't exist on Windows (tauri 2.11.5 gates it to
  `target_os = "macos"/"ios"/"android"`), so `cargo tauri build` never compiled on Windows.
  Wrapped the match arm and its `Emitter`/`Manager`/`RunEvent` imports in the same `#[cfg(any(...))]`
  guard tauri's own source uses. Verified: `cargo tauri build --debug` now succeeds on Windows
  and the resulting `confy-desktop.exe` passes a manual save-then-immediately-reload check.

### Changed
- **refactor(desktop): plugin-backed file I/O (M1 Task 1)** (2026-07-13). `crates/confy-tauri`
  no longer implements `open_dialog`/`save_dialog`/`read_file_text`/`write_file` as custom Rust
  commands — the builder now registers `tauri_plugin_fs::init()` alongside the existing
  `tauri_plugin_dialog::init()`, and `web/fs.ts` calls `window.__TAURI__.dialog.open()`/`save()`
  and `window.__TAURI__.fs.readTextFile()`/`writeTextFile()` directly (the `FsHandle` shape and
  every `ui.ts`/`touch/app.ts` call site are unchanged). `startup_file` (CLI-arg open) stays a
  custom command — no stock plugin covers it. `capabilities/default.json` gained explicit
  `fs:allow-read-text-file`/`fs:allow-write-text-file` + an unrestricted `fs:scope` (`**` —
  desktop is unsandboxed, matching normal desktop app trust; Android's real fs access instead
  routes through the Task 0 `tauri-plugin-confy-picker` plugin). No change to `confy-core`,
  `confy-ffi`, or the `Intent`/`SessionSnapshot` contract — `functional_smoke.mjs` still passes.
- **feat(web): unify Save vs. Save As / Convert across desktop and touch (M1 Task 2)**
  (2026-07-13). Desktop already split these (⌘S = instant in-place save; the toolbar Save
  button opens the "Save / Convert…" panel) — touch's single Save button used to always open
  that panel, with no quick in-place save at all. New shared `host-io.ts::doQuickSave`: writes
  straight to the open handle when one exists, else behaves like a first Save As (gated by the
  new `canSaveAs()`, see below). `ui.ts`'s `doSave()` becomes a thin wrapper over it (behavior
  unchanged, now shared instead of duplicated); touch gains a small kebab button next to Save
  (`web.toolbar.saveAs.title`) that opens the same "Save / Convert…" sheet desktop's button
  does. Also fixed while touching this code: a cancelled/failed `pickSaveFile()` was unguarded
  in all three save-destination call sites — browsers reject `showSaveFilePicker()` with
  `AbortError` on cancel (unlike Tauri's `null`), which surfaced as an unhandled rejection;
  now caught and treated as a silent cancel everywhere.
- **feat(web): `canSaveAs()` mobile guard (M1 Task 2)** (2026-07-13). New `fs.ts::canSaveAs()` —
  false only on Tauri mobile (`isTauriMobile()`, UA-sniffed) — gates picking a *new* save
  destination: `doQuickSave`'s first-save branch, `doSaveAsCopy`, and `doConvertWrite` all show
  a translated hint (`web.mobile.saveAsUnavailable`, both catalogs) instead of opening a picker.
  In-place saves to an already-open handle are unaffected — Android's real limitation is
  `ACTION_CREATE_DOCUMENT` (picking a *new* file) being untested, not writing to a file already
  open via `tauri-plugin-confy-picker`. `menu.ts`'s native menu bar also now no-ops on Tauri
  mobile (`setupAppMenu`/`rebuildMenu`), matching the pure-web build — mobile has no menu bar.
- **feat(mobile): file association + open-intent (M1 Task 3)** (2026-07-13). New
  `crates/confy-tauri/tauri.android.conf.json` (a platform-merge override, so desktop's `dmg`
  bundle registers no unwired file association) adds `bundle.fileAssociations` for
  `.toml`/`.json`/`.jsonc`/`.yaml`/`.yml`; Tauri's Android build generates the intent-filter from
  it automatically (Task 0 finding — no manual `AndroidManifest.xml` edit). `lib.rs` gains a
  second custom command, `opened_urls` (drains a `Mutex<Vec<String>>` populated from
  `RunEvent::Opened`, for the cold-start "Open with" case) plus an `"opened"` window event (for a
  warm-running app receiving another "Open with"). No new plugin needed to read/write the
  granted `content://` URI — the launch intent's own (non-persistable, session-long) grant is
  enough for `tauri-plugin-fs`'s `readTextFile`/`writeTextFile`, unlike the Task 0 picker case.
  `web/fs.ts` gains `tauriOpenedUrls()`/`onTauriOpened()`; `web/touch/app.ts`'s `main()` checks
  cold-start URLs before falling back to `?url=`/the sample, and subscribes to `"opened"` for the
  warm case, both funneled through the existing `openTauriPath`-style read path. Desktop
  (`ui.ts`) is untouched — it keeps `startup_file` only, per the plan's scope.
- **fix(mobile): device-testing fixes from M1 Task 4** (2026-07-14/15). Real
  Android device testing of the Task 3 build surfaced several bugs, all fixed: (1) a wasm
  double-free crash — `host-io.ts::replaceSession` froze the old `Session` before attempting to
  parse the new text, so a failed open left a dangling freed reference that crashed on the next
  touch; now freed only after the replacement parses successfully. (2) A cold-start file could be
  delivered twice (both the `"opened"` event and the `opened_urls()` drain fire for the same URL)
  — deduped in `touch/app.ts` via a `Set<string>`. (3) Files opened through the app's own picker
  failed to save with a permission error — `fs.ts::pickOpenFile()` was never actually wired to
  `tauri-plugin-confy-picker` on Android (still called stock `tauri-plugin-dialog`, the exact
  thing Task 0 found doesn't grant write access) and the plugin's own capability
  (`confy-picker:default`) was missing from `capabilities/default.json`; both fixed. (4) Broadened
  `tauri.android.conf.json`'s `fileAssociations` with extra MIME fallbacks per extension (`.toml`/
  `.yaml` have no IANA-registered type, so different file managers guess differently when
  resolving a file's MIME for intent matching). (5) Regenerated Android's app icon (`cargo tauri
  icon`) — it was still Tauri's placeholder logo, never regenerated after `android init`. (6) Fixed
  the toolbar rendering under the status bar — `gen/android`'s targetSdk 36 forces edge-to-edge by
  default; opted out via `windowOptOutEdgeToEdgeEnforcement` in both theme files, plus a CSS
  `env(safe-area-inset-top)` fallback. (7) Merged the Save/Save-As kebab pair into one split-button
  pill on touch, and made the `!canSaveAs` hint fire on tap (via toast) instead of relying on a
  disabled button's title, which touch screens never show. (8) The split-button pill from (7)
  turned out to render as two buttons stacked top-to-bottom on a real device — root-caused live
  by forwarding the app's WebView devtools socket over `adb` and driving the Chrome DevTools
  Protocol directly (`Runtime.evaluate` over the raw WebSocket): the touch build loads its own
  separate `web/touch/style.css`, which never received the `.split-btn` CSS rule fix (7) added
  only to the shared desktop `web/style.css`. On seeing the fix live, the split-button pill design
  itself was dropped in favor of a plain Save button that always opens a small sheet with
  "Save"/"Save As / Convert…" choices (`touch/app.ts::openSaveSheet`), keeping the underlying
  `doQuickSave`/`openSaveConvert` wiring. (9) Picker-opened files misdetected format (defaulted to
  TOML) — root cause was two layers deep: `content://` URIs are opaque (the Downloads provider's
  `.../document/msf:NNN` IDs embed no filename), and while `tauri-plugin-confy-picker`'s Kotlin
  side correctly queries the real name via `ContentResolver`'s `DISPLAY_NAME` column, the
  Rust-side `PickWritableResponse` struct only declared a `uri` field — serde silently dropped the
  Kotlin plugin's `name` key on every response before it reached JS. Fixed by adding `name:
  Option<String>` to the struct. (10) Re-confirmed the "Open with" chooser inconsistency across
  file managers from (4) is an accepted, unfixable-from-confy's-side limitation (Tauri's
  `fileAssociations` schema has no `android:scheme`, so content-URI matching depends entirely on
  each file manager's own MIME-guessing heuristics). (11) The status-bar overlap from (6)
  regressed for the same reason as (8) — the `env(safe-area-inset-top)` fix only landed in the
  shared desktop stylesheet, never mirrored into `web/touch/style.css`; added there too. (12) The
  launcher icon appeared as a solid dark block — `icons/icon.png` has zero alpha transparency
  anywhere, so Android's adaptive-icon foreground layer filled the entire icon with no margin for
  the background color to show through; disabled adaptive icons on Android (removed
  `gen/android`'s `mipmap-anydpi-v26/ic_launcher.xml` and its now-orphaned background/foreground
  resources) so it falls back to the plain flat `ic_launcher.png` mipmaps. All fixes verified live
  on a real device — several (8, 9, 11, 12) by driving the actual system UI (document picker,
  home screen) via `adb shell input tap`/`screencap` against the app's own WebView inspected live
  over CDP, rather than relying on manual user retesting for every iteration. The full M1
  acceptance flow (pick → edit → save → kill app → reopen via "Open with" → prior edit present)
  passed on real hardware. Full findings recorded in
  `docs/superpowers/plans/2026-07-13-mobile-m1-android-plan.md` (Task 4 outcome).
- **chore(docs): retire stale port-era scratch** (2026-07-13). `HANDOFF.md` removed (it froze
  at the 2026-06-18 port status and self-described as deletable once the port was done — the
  history stays in git and `PORTING.md`); `docs/tmp/`'s 178 agd dispatch artifacts archived
  into `docs/tmp-archive-2026-05.tar.gz` (gitignored). New plan:
  `docs/superpowers/plans/2026-07-13-mobile-m1-android-plan.md` (Mobile M1, Android APK).

### Fixed
- **fix(web): favicon 404** (2026-07-13). `index.html`/`touch.html` now declare
  `<link rel="icon">` pointing at the existing `icons/icon-192.png`, so browsers stop
  requesting the nonexistent `/favicon.ico`.

### Added
- **feat(web): PWA installability + offline support** (2026-07-12). `web/manifest.webmanifest`
  (standalone display, 192/512 icons derived from the desktop icon set) + `web/sw.js`, a
  network-first service worker with cache fallback: fresh deploys are picked up immediately,
  and the app shell (both UIs + wasm core) is precached on install so the site works offline
  after the first visit. Registered from `index.html`/`touch.html` on https only — the dev
  server stays SW-free so its `no-store` caching keeps working. `cf-build.sh` ships the new
  assets; `serve.mjs` learns the `.webmanifest`/`.png` MIME types.

## [v0.14.0] - 2026-07-12

### Added
- **feat(i18n): runtime language switching (English + Traditional Chinese) across TUI, web,
  and desktop.** Four-phase rollout, all now complete:
  - **core**: `Lang` enum + `tr`/`tr_args` catalog lookup (`crates/confy-core/src/session/
    i18n.rs`), reading flat `core.*`/`tui.*`/`web.*` keys from root `i18n/en.json` (canonical)
    and `i18n/zh-TW.json`, en-fallback on any missing key. `Session.lang`, `Intent::SetLang`,
    and `SessionSnapshot.lang` wire the preference through both hosts; every core-composed
    status/error/detail string routes through the catalog.
  - **TUI**: a config file (`~/.config/confy/config.toml`, `%APPDATA%\confy\config.toml` on
    Windows) persists the language choice; `--lang <code>` overrides it for one session; a new
    `l` key opens a language picker; the About screen discloses the resolved config path and
    active language.
  - **web/desktop**: `web/i18n.ts` mirrors the catalog fallback chain for TypeScript, persists
    the choice in `localStorage["confy-lang"]`, and adds a language selector next to the theme
    toggle (desktop) / in the ⋯ menu (touch).
  - **translation**: a full zh-TW pass over all ~183 catalog keys (previously only a proof-of-
    concept subset), a real zh-TW About body, and a translated `web/help-content.ts`
    keyboard-shortcut cheatsheet — canonical project vocabulary (Node/Branch/Leaf/Comment/
    Remark, KIND tags, format names) is kept in English throughout, per the i18n plan's
    glossary decision. (2026-07-11)
- **feat(desktop): enable page-zoom hotkeys in the Tauri window.** `zoomHotkeysEnabled: true`
  in `tauri.conf.json` — Ctrl/Cmd `+`/`-` and Ctrl+mouse-wheel zoom now work in the desktop
  app (native WebView2 zoom control on Windows; Tauri's 20%-step polyfill on macOS/Linux).
  (2026-07-11)

### Fixed
- **fix(i18n): lowercase `l` for the TUI language picker, add its missing help-text line, and
  turn the web language toggle into a picker.** Follow-up polish on the i18n rollout above:
  the TUI keybinding moves from capital `L` to lowercase `l` (no collision; the capital's
  K/C/E-convention rationale wasn't load-bearing); the `?` help overlay (`tui.help.toml`/
  `.json`/`.yaml` in both catalogs) gained the language-picker line it was missing since
  Phase 2; and the web/desktop and touch language controls, which used to cycle blindly
  between `en`/`zh-TW` on every click, now open an explicit picker listing all languages by
  display name (`web/i18n.ts`'s new `LANG_DISPLAY_NAMES`/`availableLangs()`) with the active
  one checked — desktop reuses the existing `.pop`/`.menu-item` click-menu machinery
  (`#langMenu`, alongside `#kindMenu`/`#moreMenu`), touch adds a dedicated `lang-sheet`
  mirroring the kind-switch sheet. (2026-07-11)
- **feat(desktop): native menu bar (File/Edit/View/Help) for the Tauri shell.** New
  `web/menu.ts` builds the menu via `window.__TAURI__.menu` (`withGlobalTauri`, no new npm
  dependency) as early as possible in `main()` — before the wasm load — so it's visible during
  the startup gap and Quit/About (`PredefinedMenuItem`) keep working even if wasm init fails.
  File gets Open/Open Recent (▸, persisted in `localStorage["confy-recent"]`, cap 8)/Save;
  Edit mixes native `Predefined` text-field items (Cut/Copy/Paste/Undo/Redo/SelectAll) with
  node-op items (Copy/Cut/Paste/Undo/Redo Node) that deliberately carry **no accelerator** —
  binding `CmdOrCtrl+C/X/V/Z/Y` would steal those keys from every text input, so the plain-key
  hint (`c`/`x`/`v`/`z`/`y`) is just a label suffix and real handling stays in `ui.ts`'s
  `onKey`; View gets Theme/Zoom (no accelerator either — `zoomHotkeysEnabled` already owns
  Cmd+/−/0)/Language (checked against `getLang()`); Help gets Help/About (both open the
  existing in-app overlay). macOS gets a rebuilt app submenu (About/Hide/HideOthers/ShowAll/
  Quit) since `setAsAppMenu()` replaces the default one entirely; Windows puts a Predefined
  Quit at the bottom of File instead. Zero `confy-core`/Rust changes — the pure-web build is
  unaffected (`isTauri()` no-ops everywhere). (2026-07-12)

### Fixed
- **fix(desktop): native menu items stopped responding after opening a file.** Every
  `Menu`/`Submenu`/`MenuItem` built in `web/menu.ts`'s `buildAndSet()` was a local variable —
  once the function returned, nothing in JS referenced them, so V8 could garbage-collect the
  tree at any later point (a big allocation spike, e.g. opening a file and swapping in a fresh
  wasm `Session`, is a classic GC trigger). GC'ing the JS wrapper tears down its Tauri
  resource — including the click-action channel — while the native OS menu bar keeps showing
  the now-unresponsive item. Fixed by keeping the root `Menu` referenced in a module-level
  variable for the page's lifetime (children stay alive via the Rust-side tree the root owns).
  (2026-07-12)
- **fix(desktop): View > Zoom In/Out/Reset threw "webview.set_webview_zoom not allowed".**
  `core:webview:default` doesn't include `allow-set-webview-zoom` — added
  `core:webview:allow-set-webview-zoom` explicitly to `capabilities/default.json`. (2026-07-12)
- **fix(desktop): macOS native menu bar was completely empty.** The macOS app submenu's
  `PredefinedMenuItem.new({ item: "About" })` sent the bare string `"About"`, but Tauri's Rust
  side models that specific predefined kind as a newtype variant carrying
  `Option<AboutMetadata>` — every other kind (`Quit`, `Hide`, …) is a plain unit variant and
  accepts the bare string, but `About` needs `{ item: { About: null } }`. The bad payload
  failed to deserialize (`invalid type: unit variant, expected newtype variant`) on the very
  first item constructed, aborting the whole menu build before `setAsAppMenu()` ever ran — with
  no visible symptom until a companion fix (same commit) turned the menu module's previously-
  silent failure paths into a surfaced status/console error. (2026-07-12)

### Changed
- **chore(desktop): macOS app-submenu "About confy" now opens the in-app About overlay** (same
  `EnterHelp`+`ToggleHelpTab` handler as the Help menu's About item) instead of macOS's native
  About panel, for one consistent About surface across platforms/content. (2026-07-12)
- **feat(desktop): File > New (`CmdOrCtrl+N`).** Discards the current document and loads the
  default built-in toml sample — the same fallback `main()` already takes with no startup file/
  URL, i.e. equivalent to refreshing the web page. No confirmation prompt, matching a browser
  refresh's unconditional discard. (2026-07-12)

## [v0.13.0] - 2026-07-10

### Changed
- **chore(ci): register Microsoft Store submission identity for the desktop `.msix`.**
  Partner Center app-name reservation for "confy" was taken; registered the Store listing
  under a distinct reserved name instead while keeping the on-disk `AppxManifest.xml`
  `DisplayName` as `confy` (Store package identity is independent of the CLI/repo/domain
  name — see `crates/confy-tauri/msix/STORE.md`). Set the `MSIX_IDENTITY_NAME`,
  `MSIX_PUBLISHER`, and `MSIX_PUBLISHER_DISPLAY` GitHub repo variables so the release
  workflow bakes the real Store identity into the `.msix` instead of the placeholder GUID.
  (2026-07-10)

## [v0.12.3] - 2026-07-10

### Fixed
- **fix(ci): CI-built Windows desktop exe showed "localhost refused to connect".** Tauri only
  embeds `frontendDist` when the `custom-protocol` cargo feature is on; `cargo tauri build`
  enables it automatically (why local builds worked) but the CI job's plain `cargo build`
  did not, so the release exe tried to load `devUrl` (`http://localhost:8080`). The feature
  is now declared in `confy-tauri/Cargo.toml` (non-default, so `cargo tauri dev` keeps the
  dev server) and passed explicitly in the release workflow. (2026-07-10)
- **docs(ci): macOS "confy is damaged" is Gatekeeper, not a broken build.** The `.dmg` is
  ad-hoc signed and not notarized (no Apple Developer ID), so downloaded (quarantined) copies
  are rejected as "damaged"; local builds run because they carry no quarantine attribute. The
  release workflow now appends an `xattr -cr /Applications/confy.app` workaround note to every
  release body. Proper fix (codesign + notarization) needs a paid Apple Developer account.
  (2026-07-10)

### Added
- **feat(ci): Rust build caching in the release workflow.** Both the TUI and desktop matrix
  jobs now use `Swatinem/rust-cache@v2` (per-target keys) to cache `~/.cargo` and the
  workspace `./target`, so dependency rlibs — the Tauri chain and the wasm32 confy-ffi deps
  in particular — are reused instead of rebuilt every run. Note: GitHub scopes caches to the
  creating ref + the default branch, and this workflow only runs on tags/dispatch, so warm
  the cache with a `workflow_dispatch` dry run on `main` first; the final fat-LTO link of
  confy itself is inherently uncacheable. (2026-07-10)

## [v0.12.2] - 2026-07-10

### Added
- **feat(ci): TUI release adds `aarch64-unknown-linux-musl`; drop the Intel-mac desktop build.**
  The TUI build matrix gains `aarch64-unknown-linux-musl` (fully static ARM Linux binary), built
  with `cross` since Ubuntu ships no aarch64-musl gcc. The `desktop` matrix drops
  `x86_64-apple-darwin`: the Intel runner took ~33 minutes for the wasm+Tauri release build vs
  ~9 for aarch64, and Apple-silicon Macs run the aarch64 `.dmg` natively (the Intel-mac *TUI*
  binary is still released). (2026-07-10)
- **feat(tauri): Windows-aware Tauri config via `tauri.windows.conf.json`.**
  `tauri.conf.json` keeps the cross-platform defaults CI needs (bash `beforeBuildCommand` →
  `web/cf-build.sh`, bundle target `dmg`); a new platform override `tauri.windows.conf.json`
  (merged automatically by Tauri v2 on Windows) empties the before-commands (bash +
  `git rev-parse` don't run under the Windows build shell — build `web/dist` manually first)
  and bundles `nsis` instead. (2026-07-10)

### Changed
- **fix(web): About version is now build-stamped, not hand-updated.** `help-content.ts`'s
  `ABOUT_TEXT` was hardcoded `confy 0.11.2`; it now uses the `__APP_VERSION__` define
  `build.mjs` already stamps from the root `Cargo.toml` `[workspace.package] version` — the
  same single source the TUI reads via `env!("CARGO_PKG_VERSION")`. (2026-07-10)
- **feat(web): Help entries are visually distinct from their descriptions.** The desktop Help
  overlay and touch help sheet render the key/shortcut column of each help and KIND-legend
  line in an accent-colored `.help-key` span (section rules dimmed via `.help-sect`) instead
  of a flat monochrome `<pre>`; column alignment is preserved. `helpBody` → `helpBodyHTML` in
  `web/help-content.ts`. (2026-07-10)
- **feat(ci): release desktop apps — macOS .dmg + Windows portable exe + Store .msix.**
  `release.yml` gains a `desktop` matrix job: macOS aarch64/x86_64 `.dmg` via `cargo tauri
  build` (bundle targets narrowed from `"all"` to `["dmg"]`), and Windows x64 as a portable
  `confy-desktop-windows-x86_64.exe` (frontend embedded at compile time; same AV-friendly
  profile overrides as the TUI exe) plus an unsigned `.msix` for Microsoft Store submission,
  packed by the new `crates/confy-tauri/msix/` scaffold (`AppxManifest.xml` with `runFullTrust`
  + config file-type associations, `pack-msix.ps1` MakeAppx wrapper, `STORE.md` Partner Center
  guide — identity comes from `MSIX_*` repo variables once registered). A `workflow_dispatch`
  trigger allows dry runs that build everything without publishing. (2026-07-10)

### Fixed
- **fix(tauri): macOS save opened the share sheet instead of a native save dialog.**
  `withGlobalTauri` was never enabled in `tauri.conf.json`, so `window.__TAURI__` was not
  injected into the webview; `fs.ts`'s `isTauri()` always returned false, and Save fell through
  to the browser download fallback, whose `navigator.share` path is supported by WKWebView —
  hence the macOS share sheet. Enabling `withGlobalTauri` restores the intended native
  save-dialog / in-place-write path. Additionally the `open_dialog`/`save_dialog` commands were
  sync, so their `blocking_*` dialog calls ran on the main thread and froze the dialog (macOS
  can't pump the run loop) — they are now `async`, running off the main thread. (2026-07-10)

## [v0.12.1] - 2026-07-10

### Fixed
- **fix(core+web): kind-switch and touch swipe polish.** JSON: the collapse-to-inline comment
  guard text-scanned the container source, so a string *value* containing `//` or `/*` wrongly
  blocked converting an object/array to inline — it now checks for real comment tokens
  (regression test added; TOML/YAML already token-checked). Touch: committing a kind switch now
  toasts the core error (e.g. "an inline table can't keep comments") instead of an unconditional
  "Kind changed"; the detail panel's disabled trailing-comment input states the reason in its
  placeholder ("inline members can't hold comments") instead of "add a comment…" (touch has no
  hover tooltip); and the red swipe-to-delete button is now `visibility:hidden` at rest and only
  revealed during/after a swipe (`.row.swiping`), eliminating the red slivers that scroll
  repaints flashed at the rows' rounded corners. (2026-07-10)
- **fix(core+web): drag/swipe/type-filter polish across desktop and touch.** Touch: pressing a
  row no longer bleeds the red swipe-to-delete button through the press-tint (the `:active`
  background was translucent; delete now only appears on an actual left-swipe), and a failed
  drag-reorder no longer arms the clipboard in paste-**cut** mode (core `move_selection_to` now
  restores the pre-drag clipboard when `do_paste`'s failure contract would have kept the
  synthetic cut fragments; regression test added). Type filter: the popup's `✕` is now a clearer
  `Clear` text button on both surfaces (shared `typefilter.ts`), and a new
  `SessionSnapshot.type_filter_active` flag drives the toolbar funnel button's active state on
  both UIs — fixing the touch button staying lit after Clear (its old sticky proxy only updated
  while the popup was open) and giving desktop the same `.on` + dot indicator as touch.
  (2026-07-10)

### Changed
- **chore(web): deduplicate desktop/touch orchestrators into the shared modules.** A web-UI
  consistency audit found `web/touch/app.ts` re-implementing flows the shared modules already
  own; it now imports them instead: the host I/O flows (`host-io.ts` — save-copy, convert-write,
  Save/Convert panel open, open-from-URL, format sniffing, theme) via a touch `HostIo` adapter
  (toast/status feedback, close-sheets-before-convert, FxiOS download hint), the built-in welcome
  sample + sample-mode state (`samples.ts`), and `parentOf`/`siblingIndex` (`path-utils.ts`) —
  ~270 duplicated lines deleted, and touch gains the same batched-dispatch (`batch`) single-render
  behavior as desktop. The kind-badge label (friendly `KIND_SHORT` name + notation glyph, e.g.
  `str·"…"`) moved into a shared `kindLabelParts` in `kind-labels.ts`, so the touch tree's kind
  badge now shows the same label + notation suffix as desktop instead of the raw `type_label`.
  Also removed `ui.ts`'s never-called inert touch-scaffolding stubs (superseded by the real
  `web/touch/` UI). As a side effect of adopting the shared `doConvertWrite`, touch's convert
  Save-As picker now receives the *target* format (desktop behavior) rather than the source
  format. (2026-07-10)

## [v0.12.0] - 2026-07-09

### Added
- **feat(core+tui+web): shared tabbed Help/About panel.** A single `Mode::Help { tab }` (core
  `HelpTab`) backs the TUI's `?` overlay (`Tab` key toggles Help/About) and both Web UIs; the
  desktop and touch UIs add a header info button that opens the same panel, with tab buttons
  matching the TUI's toggle. Fixed alongside: the desktop `#overlay` clipping under the header
  (z-index/positioning) and a unified Open popup (local file + URL, one surface) on both desktop
  and touch. (2026-07-09)

### Fixed
- **fix(web): touch/desktop Open+Save panel polish and focus/scroll fixes.** Touch: fixed a
  scroll-jump where opening the Open/Edit sheet's autofocused input triggered a `scrollIntoView`
  that shifted the whole `.app` shell (`position:absolute`, scrolls with the page), exposing the
  sheet stacked underneath it — the Open sheet now explicitly focuses Cancel instead of the URL
  input; hid the swipe-to-delete button during grip-drag reorder (the drag-dim opacity on
  `.row-main` was letting it show through); wrapped Help/About body text instead of scrolling it
  horizontally. Desktop + touch: restyled "Browse local file" as an icon+label action card, and
  matched touch's Open/Save/Edit modal button order and styling on desktop (Cancel first, primary
  action last and blue-filled); stopped native `showModal()` from autofocusing the Save dialog's
  Format select. (2026-07-09)
- **fix(web): tab-click no-ops on the already-active Help/About tab.** Both web UIs wired *both*
  tab buttons to a blind `send("ToggleHelpTab")`, so clicking the already-active tab flipped away
  from it instead of doing nothing; `web/ui.ts` and `web/touch/app.ts` now compare the clicked
  button's `data-tab` against the active tab before sending. Body composition for the two surfaces
  was also hoisted into a shared `helpBody()` in `web/help-content.ts`. (2026-07-09)
- **fix(web+core): branch-key rename, leftover Esc-cancel flag, touch empty-tap.** Three follow-up
  repairs found in manual testing of the above panel/prompt work (2026-07-09):
  - **Renaming a branch (table) node's key from the Detail panel raised `invalid value: expected
    value (…)`** (`session.rs::commit_edit`): the Web `CommitEdit` path always seeds `Mode::Edit`
    via `begin_inline_edit` (`rename_only: false`), so after a successful rename it fell through to
    the value-replace step and tried to reparse the branch's (empty) value buffer as a scalar.
    `commit_edit` now sets `rename_only` when the cursor is on a branch, matching the TUI's `F2`
    rename-only path. Regression: `dispatch_commit_edit_renames_branch_key`.
  - **Esc-cancel on a detail-panel input could swallow the *next* legitimate edit** (`web/panel.ts`
    `commit`): restoring the input to its focus-time value already suppresses the browser's own
    `change` event (nothing to commit), so the added `cancelled` bookkeeping was redundant — and
    wrong, because that flag was only reset *inside* the `change` handler, which Esc's restore never
    fires. The next real edit's commit landed while `cancelled` was still `true` and got silently
    dropped, requiring a second attempt. Removed the flag; Esc still restores + blurs, but relies on
    the native no-change-fires-no-commit behavior instead of tracking it manually.
  - **Tapping empty tree space on touch did nothing** (`web/touch/app.ts`): desktop's `onTreeClick`
    clears the multi-select and any error banner when a click misses every `.row`; the touch pointer
    gesture handlers only ever ran when a `.row` was found, so the equivalent tap (in the
    `.tree-pane` padding below the last row) was silently ignored. Added a plain `click` listener on
    `.tree-pane` for the same behavior.
  - **Desktop type-change confirm popup flashed shut instantly instead of waiting for an answer**
    (`web/panel.ts` `commit`): the Enter-to-blur keydown handler didn't stop propagation, so after
    `el.blur()` synchronously committed the edit and opened `Mode::Prompt`, the *same* keydown event
    kept bubbling — past the input (now blurred, so `ui.ts`'s "don't hijack text entry" INPUT-tag
    guard on its `document.body` keydown listener no longer matched) — up to the global `onKey`,
    whose Prompt handling treats Enter as "y". The prompt was answered before the browser ever
    painted it. Enter now `stopPropagation()`s like Escape already did.
- **fix(web+core): confirm-prompt buttons, detail-panel edit repairs, dynamic inline-editor width.**
  Five web-UI repairs, two of which were core bugs (2026-07-08):
  - **TOML rename under a `[section]` returned "path not found"** (`cst_edit.rs::rename`): the
    absolute path segment position was used directly as the KEY-token index, but a scoped entry's
    key spells only its own tail — every rename of a key inside a `[table]` failed (TUI `F2`
    included). Entry indices are now end-relative and header indices skip `Seg::Index` slots;
    regression tests cover scoped scalars, scoped dotted leaves/intermediates, and AoT-adjacent
    sub-headers. The cursor now also *follows* a renamed node instead of snapping to the first row.
  - **`CommitEdit` is now truly one-shot** (`session.rs::commit_edit`): a retry branch (invalid
    value, rename failure, …) used to leave a dangling `Mode::Edit` that the desktop web UI rendered
    as a focused tree inline editor while the detail panel vanished. It now cancels the edit,
    surfaces the message as `error`, and — when the commit originated from the Detail panel —
    returns to `Mode::Detail`, so panel edits keep the panel open (type-change prompts resolve back
    to Detail on both answers via the new `prompt_from_commit_edit` flag; `n` no longer restores
    `Mode::Edit` for pointer hosts).
  - **y/n confirm prompts are now buttons** on both web UIs (shared `web/prompt.ts`): the desktop
    `#overlay` renders per-kind answer buttons (TypeChange/ArrayUpgrade/JsoncUpgrade/ConfirmQuit,
    plus Overwrite/Rename/Cancel for Collision — previously unreachable by keyboard on web), and
    the touch UI gains a prompt bottom sheet — before this, `Mode::Prompt` soft-locked the touch UI
    (no surface, no keyboard), which is why a type-changing value edit "did nothing" on touch.
    Scrim/× dismissal answers `n` (peel-on-dismiss), never just hides the sheet. The desktop detail
    panel stays open underneath a prompt.
  - **Esc now cancels detail-panel inline edits** (key/value/trailing/comment inputs in
    `web/panel.ts`): restores the original text and swallows the blur-commit; tree inline editors
    already supported Esc.
  - **Desktop inline editors size to their content** (`web/render.ts` `editWidthCh` + auto-grow on
    input in `ui.ts`): value/name/comment inputs open at the text's own `ch` width and grow while
    typing, still clamped by the existing CSS min/max-width.

### Added
- **feat(web): open config from URL via `?url=` deep-link.** Appending `?url=<encoded-url>` to
  the page URL fetches and opens that remote config at boot (priority: Tauri startup file →
  `?url=` → built-in sample). Format is inferred from the filename extension, falling back to the
  HTTP `Content-Type` header (default: TOML). No on-disk handle is held, so Save degrades to Save
  As / download — identical to the file-input path. `fetchUrlFile` (`web/fs.ts`) and
  `openFromUrl` + `formatFromNameOrType` (`web/ui.ts`) are the new entry points. (2026-06-28)
- **feat(web): explicit "Open from URL" entry point.** Beyond the `?url=` deep-link, the desktop UI
  adds an "Open from URL…" item to the ⋯ More menu (opens a `#url-modal`) and the touch UI adds an
  "Open from URL" row to the More-actions sheet (opens a `.url-sheet`); both feed the same
  `openFromUrl`. The existing local-file Open button is untouched. (2026-06-28)
- **feat(desktop): Tauri v2 desktop app (`confy-desktop`).** New `crates/confy-tauri` wraps the
  existing web UI in a native desktop shell (macOS + Windows; Linux not targeted yet). Editing stays
  in the in-webview wasm `Session` — `dispatch` is synchronous and called from ~100 keyboard handlers,
  so it is *not* moved across the async IPC boundary (route "B-lite"). The Rust side owns only the
  part that genuinely needs the desktop: **native file I/O** — real open/save dialogs, in-place writes
  to an arbitrary path, and opening a file passed on the command line — via 5 `#[tauri::command]`s
  (`open_dialog`/`save_dialog`/`read_file_text`/`write_file`/`startup_file`) and the dialog plugin.
  `web/fs.ts` detects Tauri (`window.__TAURI__`) and wraps the file path in an object conforming to
  the existing `FsHandle` shape, so `ui.ts` is unchanged and the browser File System Access API path
  is fully preserved. Build with `cargo tauri build` from `crates/confy-tauri` (Windows must be built
  on a Windows host). (2026-06-27)

### Changed
- **chore: audit-driven optimization pass.** A workspace-wide read-only audit produced a ranked list
  of correctness/perf/dedup findings; this lands the verified set (build + clippy + `tsc` + web bundle
  all green). **Correctness:** JSONC comment-capability is now derived from the lexer token stream, not
  raw `text.contains("//")`, so a `//` inside a JSON string value no longer silently enables comments
  (A1); a float→float `K` switch to plain notation keeps its `.0` so `1.5e3` renders `1500.0` rather
  than being reclassified as Integer (B9); JSON remark/edit-comment resolve the target by node identity
  instead of text equality, so duplicate-text siblings mutate the right node (B10); the Web
  `dispatch()` now snaps the cursor onto a visible row after a structural change, mirroring the TUI
  (B11). **Performance:** `NodeTree::node_at` descends segment-by-segment (O(depth)) instead of
  scanning the whole tree (A2); YAML/JSON `apply` threads a single projection/index instead of
  re-projecting per lookup (A4); `SkimMatcherV2` is built once via `LazyLock` instead of per fuzzy call
  (B2); ~30 Session methods use a borrowed `visible_nodes()` instead of cloning the full row vec (B1);
  the TUI's `rebuild_rows` drops a redundant per-row tree lookup, reusing `ViewRow` fields (B3);
  `is_dirty()` short-circuits via a `clean` flag instead of serializing the whole document on every
  snapshot (B5); the inline editor builds ≤3 style spans instead of one per character (I4).
  **Dedup / cleanup:** dead `Update` transport struct removed; ~170 lines of test-only copies of core
  helpers deleted from the TUI (their tests moved to core, A5); the TUI's `type_tag` now maps the
  shared `classify` decision table instead of duplicating it (B15); `copy_selected`/`cut_selected`
  merged into one `capture_selected` (B17); the YAML byte-splice tail extracted into one
  `commit_reparse` helper across 10 sites (B14) and the projection's comment accumulator into one
  `CommentAccumulator` across the three walkers (B16); YAML collision-rename aligned to `{key}_{n}`
  like TOML/JSON (C5); the unreachable Web paste-load modal deleted; shared Web modules extracted
  (`escape.ts`, `samples.ts`, `host-io.ts`, `path-utils.ts`, `kind-labels.ts`) to de-duplicate the
  desktop/touch orchestrators; touch trailing comments no longer render a doubled comment marker (B8);
  the convert Save-As picker uses the target format's filter (B12); several stale "stub/not-yet-ported"
  module comments corrected (C2). Deferred as net-negative to force without runtime verification:
  touch-side `host-io.ts` adoption (untested web UI), and the pure-readability splits of `edit_commit`
  (C7) / `insert` (Q3). (2026-07-08)

## [v0.11.2] - 2026-06-27

### Changed
- chore: version-bump-only release to trigger the Cloudflare Workers Build. No
  functional changes since v0.11.0; cuts a release commit so a `web/package.json`
  build-watch-path fires the deploy. (2026-06-27)

## [v0.11.0] - 2026-06-27

### Fixed
- **fix(core): a comment dropped into a non-last TOML `[table]` now lands inside it.** A standalone
  comment between a table's last entry and the next `[header]` was always projected as the *next*
  section's leading comment (attached to root), so a touch/desktop drop "into" a collapsed table
  appeared *after* the branch. Projection now uses a **blank-line rule**: a comment separated from
  the following header by a blank line trails the preceding table; a comment hugging the header
  stays its leading comment. The drop/insert path emits that separating blank line when a comment is
  appended right before an outer header. JSON/YAML were unaffected (explicit `}`/dedent delimiters).
  Round-trip stays byte-identical (serialize is unchanged); regression + projection tests added.
  (2026-06-27)
- **fix(core): keep the selection on a moved *comment* after a downward reorder.** Follow-up to the
  v0.10.1 node fix: the post-move selection subtracted only the removed node sources, not removed
  comment sources, so a downward comment move still cursored the next row. Subtract `comment_shift`
  too. (2026-06-27)
- **fix(web): iPhone file-open picker no longer greys out `.toml`/`.yaml`.** iOS resolves the
  `<input accept>` extensions to UTIs and disables those without one; the `accept` filter is dropped
  so any config file is selectable (parse rejects non-config content). Desktop Chromium uses the FS
  Access API, unaffected. (2026-06-27)
- **fix(web): best-effort Web Share + a Firefox-iOS save hint.** `downloadText` now *attempts*
  `navigator.share({files})` whenever `share` exists — including when `canShare` is absent — and
  falls back to the anchor download only on a non-cancellation rejection, so the filename/extension
  survive into "Save to Files" where supported. Firefox iOS exposes no file share *and* WebKit
  ignores the `<a download>` name (extension comes from the MIME type, and iOS has no UTI for
  `.toml`/`.yaml`), so a downloaded `.toml`/`.yaml` is extension-less there with no possible fix on
  the anchor path; the touch UI now shows a one-time toast hinting the user to open the site in
  Safari (which works via Web Share). (2026-06-27)

## [v0.10.1] - 2026-06-27

### Fixed
- **fix(core): downward drag-reorder no longer leaves the selection on the next row.** `do_paste`'s
  post-move selection indexed the rebuilt child sequence with the raw `target.index`; on a
  same-parent **downward** move the earlier source is removed first, shifting the landing slot up by
  that count (the Move/Insert mutations already account for it via `node_shift`) — the selection did
  not, so the moved node's *next* sibling got selected/cursored. Subtract `node_shift` from the
  selection start. Upward / cross-parent / paste (non-cut) unaffected (`node_shift == 0`). Fixes the
  Web UI grip drag-reparent and TUI cut-paste (shared core); regression test added. (2026-06-27)

### Added
- **Web UI hosted at <https://confy.turkeyang.net/>** via Cloudflare Workers Builds Git
  integration: `web/cf-build.sh` (build command — installs Rust/wasm-pack if absent, builds the
  wasm core + TS bundle, assembles a runtime-only `web/dist`) and root `wrangler.toml` (deploy
  command `npx wrangler deploy` → assets-only Worker `confy` serving `web/dist`). Deploys on every
  push to `main`; `web/dist` gitignored. README gains a **Web UI / live-demo** section; WEBUI.md
  gains a **§Deployment**; CLAUDE.md module map notes the deploy files. (2026-06-27)

## [v0.10.0] - 2026-06-27

### Fixed
- **fix(web): iOS save produces a random-named `.txt` instead of the real filename:** on iPhone Safari there is no FS Access API, so save fell through to the anchor-download fallback — but iOS Safari ignores the `<a download>` filename (naming the file after the blob UUID) and appends `.txt` from the hard-coded `text/plain` MIME, losing both the name and the `.toml`/`.json`/`.yaml` extension. `downloadText` (`web/fs.ts`) now sets the correct MIME per extension via `mimeFor`, and when the host can share files (`navigator.canShare({files})`, i.e. iOS Safari) routes through the **Web Share API** so the `File`'s name and extension survive into "Save to Files"; share cancellation is swallowed. Desktop Firefox/Safari keep the anchor-download path. Web bundle clean; no core/ffi change. (2026-06-27)
- **fix(web): detail-panel edit bugs + touch overflow (5th browser-test feedback):** (1) **trailing-comment edit no longer fails with "invalid fragment"** — `Session::set_trailing_comment` now normalizes the Web panel's raw text (it expected the comment *with* its marker), prepending the backend's prefix (`#`/`//`) when missing and treating empty as a clear; previously TOML/JSON rejected raw text and YAML silently appended it as a bare token. (2) **panel Key/Value edits now take effect reliably** — the shared `web/panel.ts` commit handlers read the input value **before** the `SetCursor` dispatch that rebuilds the panel DOM (reading the detached input afterward could no-op the edit). (3) **touch:** a long / deeply-indented **key now truncates** (ellipsis, shrinking last after value then comment) instead of overflowing the row's right edge, and the **detail sheet title for a comment node** is a fixed `Comment` label (long keys ellipsize via `.sheet-head h3`) instead of dumping the whole multi-line comment as the heading. Core `confy-core` tests + `confy-ffi` smoke + both web bundles clean; frozen desktop files unchanged.

### Changed
- **feat(web): touch + desktop UI convergence round 4 (4th browser-test feedback):** detail-panel polish + touch row gestures; no core/ffi change. **Shared `web/panel.ts` (both UIs):** (1) after a successful **Delete / Copy / Cut** the panel confirms via a host message and **dismisses** (new optional `afterMutation` callback — desktop `ExitDetail`, touch `closeSheets`); (2) a **multi-line value/comment preview is truncated to the cell** (ellipsis) instead of overflowing; (3) the **Kind button shows `type · «notation»`** again (short glyph — `string · "…"`, `integer · 0x`, `table · dotted` — ported from `render.ts`, suppressed when it would just repeat the label). `web/style.css`/`web/index.html`/`web/render.ts` byte-unchanged (truncation rides inline styles in `panel.ts`). **Touch only:** (4) rows gain a **left-swipe-to-delete** gesture — sliding a row left reveals a single Delete action (`.row-del`; one open at a time; the horizontal-swipe axis lock coexists with grip-drag reorder and vertical scroll; read-only rows opt out); (5) the **Tree | Raw tabs collapse to one toggle button** (`.viewtoggle`, label = the view it switches to) that still folds into the `⋯` menu. All `tsc --noEmit` + both esbuild bundles clean; frozen desktop files show no diff. (2026-06-27)
- **feat(web): touch + desktop UI convergence round 3 (3rd browser-test feedback):** regression fixes + deeper convergence from on-device testing; no core/ffi change. **Shared modules:** `web/convert-dialog.ts` is decoupled from the `<dialog>` API behind a small `ConvertSurface` (`isOpen/open/close/onCancel`) so the same Save/Convert form can be hosted in a native dialog (desktop) **or** a bottom sheet (touch); `web/panel.ts` swaps the single **Duplicate** button for **Copy + Cut** (`CopySelected`/`CutSelected`), adds **mouse-wheel value adjust** on the panel's value field (`Bool` toggles, `Integer`/`Float` nudge ±1), and renders a **multi-line value as a button** that opens the host popup editor (`BeginEdit` → external edit) — all three apply to **both** UIs. **Desktop:** a **double-click on a row now _toggles_ the Detail panel** (was open-only); the panel's wheel-adjust + multi-line-value→popup land via the shared panel; the convert dialog adopts the dialog-backed surface (behavior unchanged). `web/style.css`/`web/index.html`/`web/render.ts` byte-unchanged. **Touch:** (1) the toolbar/filter buttons now **collapse responsively** into the `⋯` menu right→left via `@container` breakpoints (viewtabs → expand/collapse → undo/redo/theme), and the `⋯` menu is **built dynamically** from whichever controls are currently folded (no hardcoded list); (2) the **Open button works again** (its shell handler `case "open"` was missing); (3) **Save/Convert is now a bottom sheet** like every other touch panel (shared form via `ConvertSurface`); (4) the type-filter sheet drops its redundant **Done** button (the shared grid toggles live + has ✕ clear); (5) the detail-panel **kind label is centered**; (6) the row **drag grip is flush-right on every row** (branch rows had regressed); (7) panel **Copy/Cut arm the FAB** — it shows a paste glyph tinted by copy vs cut and pastes on tap, and the status-bar clipboard badge clears it; (8) the **multi-line editor is a proper in-`.app` sheet** that triggers correctly and **dismisses cleanly** (scrim/grab now `Escape`-peel core's pending edit, so it no longer re-pops and gets stuck). **Follow-up fixes (same round):** the shared panel now detects a **multi-line value by core's format rule** (`MultilineBasic`/`MultilineLiteral`/`LiteralBlock`/`Folded`, not just an embedded `\n`) so its value field reliably opens the host popup editor (desktop = centered `#ext-modal`; touch = bottom `.ext-sheet`); the panel's **wheel bool-toggle uses `Nudge`** instead of `CommitEdit` so adjusting a boolean no longer closes the Detail panel; and the touch **tree + type-filter sheet preserve their scroll position** across re-renders (a tap/cell-toggle no longer snaps the view back to the top). A **multi-line comment node** in the panel also opens the popup editor (button → `BeginEdit` → external edit, routed to `ApplyEditComment`). The **touch multi-line editor was rebuilt** as a dedicated touch-native bottom sheet (its own `.ext-text` styling + working Apply/Cancel wired directly, built fresh per edit with a re-render guard so input isn't clobbered) — replacing the borrowed desktop-modal look that was unstyled and had dead buttons; it coordinates with the other sheets via `openSheet`/`dismissSheets` (open closes the rest; dismiss `Escape`-peels) and never invokes the desktop `#ext-modal` (which only exists on the desktop page). **Touch paste-mode UX:** a small **✕ floats above the paste FAB** to clear the clipboard / exit paste mode, and the mode now **highlights the cursor row as the live paste target** (`.app.paste-mode .row.cursor`) while de-emphasizing the frozen source selection — a tap in paste mode moves only the target (`SetCursor`), so the highlight follows the destination instead of staying stuck on the source. All `tsc --noEmit` + both esbuild bundles clean; frozen desktop files show no diff. (2026-06-27)
- **feat(web): touch + desktop UI convergence round (2nd browser-test feedback):** the touch UI converges on the desktop design where its bespoke chrome was weaker, via new single-source shared modules, plus targeted desktop gesture changes. No core/ffi change. **Shared modules (NEW):** `web/convert-dialog.ts` (the native `<dialog id="convDlg">` Save / Convert panel — `renderConvertDialog`/`wireConvertDialog`/`runSaveConvert`/`extForTag`) and `web/typefilter.ts` (`typeFilterHTML`+`wireTypeFilter` for the type-filter grid) were extracted from desktop's working `ui.ts` and are now called by **both** UIs (joining `web/panel.ts`); each emits desktop's class names so `web/style.css`/`web/index.html` are byte-unchanged and desktop behavior is identical (delegating edits only). **Desktop:** a **double-click on a row now opens the Detail panel** (was branch-expand / boolean-toggle; expand stays on caret + Enter); **mouse-wheel over a value cell** adjusts it in place (`Bool` toggles, `Integer`/`Float` nudge ±1) with page-scroll preserved off-value (keyboard `+`/`-`/`←`/`→` Nudge unchanged); and the `⋯ More` overflow menu no longer lists **Save / Convert** (it lived both there and on the always-visible Save button — now Save-button only). **Touch:** (1) the header + search bar were rebuilt to **mimic desktop's** toolbar/filterbar (the bespoke app-bar was dropped; CSS ported into the app-only appendix); (2) **type filter** and **Save / Convert** now use the shared modules (the bespoke filter/convert sheets are gone); (3) the **Save button opens the Save / Convert panel** — there is no direct-save button (all saves route through the panel) and the more-menu's Save/Convert item was removed; (4) the built-in **sample** boots as the desktop's three-dialect welcome doc (named `sample`), and the **format pill cycles TOML→JSON→YAML** in sample mode (frozen once a real file is opened/saved) instead of opening convert; (5) the detail-panel **kind button renders on one line** (was wrapping the dot + label onto two). All TS typechecks (`tsc --noEmit`) and both esbuild bundles build clean. (2026-06-26)
- **feat(web): touch + desktop UI optimization round (post browser-test feedback):** a sweep of refinements from in-browser testing of the dedicated touch UI, plus a shared edit/detail panel unifying both UIs. No behavioral core change beyond one additive view-model field. **Shared panel (`web/panel.ts`, NEW):** a framework-free `panelHTML(row)` + `wirePanel(container,row,send,openKind,onError)` renders the node edit/detail panel from a `ViewRow` for **both** the touch edit sheet and the desktop detail aside, locking one field order **Key / Value / Trailing comment / Kind / Path / Children / Sign**. This fixes three touch bugs at once: the **Delete/Duplicate buttons were dead** (rendered but never wired — they fell through to a no-op), **Path showed the raw `JSON.stringify(path)`** (now the human dotted/bracketed form, e.g. `servers[1].port`), and **failures were silent** (`wirePanel` now surfaces `snapshot.error` via a toast after every dispatch). The kind button drops the long "· switch notation" suffix that broke layout. To feed the panel's Sign field, core's `ViewRow` gains an additive `key_sign: String` (`"bare"|"quoted"|"dotted"|"none"`, reusing the TUI detail-text mapping via a shared `key_sign_label` helper). **Desktop:** the detail `<aside>` now renders this shared panel **reactively** (tracks the cursor row each snapshot, fully editable) instead of the static `detail_text` `<pre>` (kept only as the empty-doc fallback). **Touch:** (1) main-button icons shrunk to ~70% and the FAB to 44×44 (was an oversized 60), searchbar/tabs to ~80%; (2) the **FAB is context-aware** like the TUI `a` — expanded branch → `AddChild`, else → `AddSibling`; (3) the initial sample is now the **same welcome sample as desktop**; (4) the format pill is a button → `OpenConvert`; (5) **swipe-to-reveal-actions removed entirely** (markup/CSS/pointer branch) — grip-drag reorder stays; (6) **single tap = select, double tap = open the edit panel** (was single-tap-opens), and the kind badge tap now selects (kind switch only in the panel); (7) the right-side branch `>` chevron removed; (8) the type-filter sheet rebuilt — scrollable body, real padding, well-formed grouped chips; (9) on `≥600px` the persistent side pane gains a **draggable splitter** (`--detail-w` flex-basis, clamped ~240–520 px, persisted to `localStorage`). `cargo test`/clippy `-D warnings`/fmt, `wasm-pack build`, esbuild bundle (`ui.js` + `touch/app.js`), and `tsc --noEmit` all clean. (2026-06-26)
- **feat(web): dedicated prototype-faithful touch UI (`web/touch/`), shared core:** a first attempt that bolted touch sheets/FAB/swipe onto the desktop chrome (gated on `pointer:coarse`) was reverted as low-fidelity — on touch it was still the desktop layout with sheets glued on. Replaced by a **separate touch UI** that ports the prototype (`docs/superpowers/specs/2026-06-26-web-respons-migrate-to-touch-ready.html`) **verbatim** in look & gesture but drives the **same `confy-core` Session** through the shared `confy.ts`/`Intent` contract — exactly how the desktop UI relates to the core. No Rust/WASM change. **Entry selection — one URL, two pages:** `index.html`'s `<head>` runs a tiny pre-paint router (`?ui=desktop` stays; `?ui=touch` or `matchMedia('(pointer:coarse)')` → `location.replace('touch.html')`); `touch.html` carries the reverse guard. A two-page redirect (not in-page DOM-swap) is used because the desktop `body{flex-direction:column}`+`.main{flex:1}` assume toolbar/main/footer are direct body children — wrapping them would break layout and force edits to the verbatim desktop CSS. **New files:** `web/touch.html` (shell + redirect), `web/touch/style.css` (the prototype's `<style>` **verbatim**, minus the showcase device-frame rules `.stage`/`.device`/`.os-status`; `body` fills the viewport and `.app` inset 46px→0 since the fake OS bar is gone), `web/touch/render.ts` (pure `SessionSnapshot → HTML`, every row a real `ViewRow`, flat visible-row list, `data-path` attribute-safe), `web/touch/app.ts` (orchestrator: boots the Session, generates the ported shell, re-points every gesture to one Intent + full re-render — stateless). **Gesture → Intent:** caret→`SetCursor`+`ToggleExpand`; row tap→`SetCursor`+`SetSelection`+Detail; kind badge→sheet from `session.kindOptions`→`CommitKind`; grip drag→`MoveSelectionTo` (sibling index = visible position, as `dnd.ts`; own-subtree excluded); left-swipe→Edit / Dup (`CopySelected`+`Paste`) / Delete; Detail key/value→`CommitEdit`, trailing→`SetTrailing`, comment node→`ApplyEditComment`; type-filter & convert sheets built from `snapshot.mode` (`TypeFilterView`/`ConvertView`); FAB→`AddNode` (parameterless — the prototype's add-type sheet dropped); search→debounced `SetFilter`; Tree/Raw view toggle; read-only/opaque rows reject edits & hide grip/kind/swipe; multi-line edits route to an external-edit sheet (`ApplyReplace`/`ApplyEditComment`). `web/build.mjs` emits both bundles (`ui.js` desktop unchanged + `touch/app.js`). `tsc --noEmit` + esbuild bundle clean; desktop `ui.ts`/`render.ts`/`style.css` byte-unchanged. (2026-06-26)
- **Web: remaining popups fit narrow/short screens:** `.pop` context/kind/more menus capped at `min(280px,92vw)` + `max-height:calc(100vh-16px)` w/ scroll; `#convDlg` gains `max-height:calc(100vh-32px)`+overflow; `.detail` aside `width:min(320px,92vw)`; CSS-only. (2026-06-26)
- **Web: popup editor modals (`#ext-modal`/`#load-modal`) now shrink to fit narrow/short screens:** `.modal-box` capped at `min(720px,92vw)` + `max-height:calc(100vh-32px)`, textarea flexes to box width (`width:100%`) overriding the fixed `cols=72`; CSS-only, no JS change. (2026-06-26)
- **Web: RWD/touch foundation — capability-gated touch tokens & hooks, shortcut tooltips, inert sheet/FAB scaffolding; no visual change:** three-part foundation layer for a future touch-first UI, additive only. (A) Shortcut tooltips: `Expand all (9)` and `Collapse all (0)` titles on their toolbar buttons; `buildMoreMenu()` now appends concise key hints to Save/Convert, Undo, Redo, Expand all, Collapse all. (B) CSS architecture: `--hit:44px` and `--row-h-touch:44px` tokens added to `:root`; width breakpoints reorganised with labelled comment headers (bytes unchanged); new `@media (pointer:coarse)` and `@media (hover:none)` blocks raise hit areas and persist row-actions on wide-screen touch — purely additive, existing mouse rules untouched. (C) Inert scaffolding: `#touch-sheet`/`#touch-fab`/`#touch-swipe` mount points added to `index.html` as `.hidden`; `initTouchScaffolding()` in `ui.ts` gated behind `matchMedia('(pointer:coarse)')` with early-return stubs — no live UI or behavior change. Web typecheck + esbuild bundle clean. (2026-06-26)
- **Web UX: dynamic sample version, toggleable menus, universal Esc (web-only):** three polish items, no core/WASM change. (1) **Built-in sample `about.version` is now build-stamped** — `web/build.mjs` reads `version` from the workspace `Cargo.toml` and injects it via esbuild `define: __APP_VERSION__`, which the three sample docs interpolate, so the demo tracks the real release instead of a stale `0.7.0` literal (falls back to `"dev"` when the bundle is loaded without the define). (2) **All menu buttons toggle** — the `⋯` More button and the per-row `⋮` context menu now close on a second click (mirroring the already-toggling type-filter button and kind badge); a new `ctxMenuPath` tracker drives the row-menu toggle. (3) **Every popup supports Esc** — added an Esc-to-close handler for the load-modal (the one surface that lacked it; all other menus/dialogs/overlays/editor already cancelled on Esc). Web typecheck + esbuild bundle clean. (2026-06-25)

## [v0.9.0] - 2026-06-25

### Fixed
- **AddSibling Escape cancellation for container nodes:** pressing Escape after "Append sibling" on a branch node (table/array) now removes the just-added container sibling — matching AddChild's existing behaviour. Previously, container siblings did not enter Edit mode, so the `created_on_add` cancellation mechanism never fired. The fix enters rename Edit mode after inserting a keyed container sibling. (2026-06-25)
- **Comment append-sibling now enters the inline editor with Escape-cancel:** appending a sibling to a comment node (TUI `a` / web "Append sibling") now inserts a *separate* single-line comment (blank-line separated, so it no longer silently merges into the adjacent comment as an invisible extra line) and immediately opens it in the inline editor; pressing Escape removes the just-added comment (and its blank separator) via the `created_on_add` → `History::cancel_last` path, matching scalar/container add. The JSON/JSONC and YAML `insert_comment` validators were relaxed to allow a blank line in the comment fragment (TOML already did). All three backends covered by headless `dispatch` tests. (2026-06-25)
- **Web: comment-row click target no longer over-wide:** a standalone comment row's text span was `flex:1 1 auto`, stretching across the whole remaining row width so clicking the empty area still opened the editor; it is now `flex:0 1 auto` (no grow, shrink retained), so only the text itself triggers editing while the narrow-width ellipsis still works. (2026-06-25)

## [v0.8.0] - 2026-06-25

### Changed
- **Web UI: Enter toggles multi-selection, narrow-width ellipsis (web-only):** two follow-up refinements, no core/WASM change. (1) **`Enter` toggles every selected branch** — with a multi-selection, `Enter` now expands/collapses each selected branch independently (cursor-walks the selected branch rows, dispatching `ToggleExpand` per row, then restores the selection); a single/empty selection keeps the plain cursor toggle. (2) **Narrow-width rows compress instead of wrapping or vanishing** — removed the `@680px` `.row{flex-wrap:wrap}` (long rows no longer spill onto a second line) and `.comment{display:none}` (comments no longer disappear); `.val` and `.comment` gain `min-width:0` so the existing `text-overflow:ellipsis` actually triggers in the flex row, and a standalone comment row's `.comment` is `flex:1 1 auto` so it fills and truncates. The **value compresses first** while the **key keeps its full width** (`.key{flex-shrink:0}`, only truncating past its `max-width:38vw` cap). Full text stays available in the detail panel (`i`). Web typecheck + esbuild bundle clean. (2026-06-25)
- **Web UI: unified Save/Convert panel, responsive toolbar, double-click toggles (web-only):** follow-up UX polish on the web-native UI, no core/WASM changes. (1) **Unified Save / Convert panel** — the separate Convert button is gone; the toolbar **Save** button opens one `#convDlg` panel whose format `<select>` defaults to the current format with the filename prefilled from the open file's stem (same format → a faithful `serialize()` "Save copy"; a different format → the existing convert/warn/confirm flow). `⌘S` stays the instant in-place save. (2) **Responsive overflow** — as the window narrows, the secondary toolbar/filter controls fold into one `⋯ More` popup one group at a time, right→left, via staged media queries (Tree/Raw ≤600px, Expand/Collapse ≤500px, Undo/Redo/Theme ≤440px); the search box gained `min-width:96px` so it yields space before they collapse. (3) **Right-click syncs selection** — opening a row's context/⋮ menu selects that row (unless it is already in the multi-selection), so menu operations never target a different node than the one clicked. (4) **Double-click toggles** — double-clicking a row's *empty* area toggles a branch's expand/collapse or a boolean leaf's value (manual two-click timing, since native `dblclick` is unreliable after the first click re-renders; only empty-space clicks reach it). (5) **Arrow-key scroll fix** — navigation keys (`←→↑↓`, Home/End, Space) now `preventDefault` and `.main` is `overflow:hidden`, so arrow keys no longer horizontally scroll the off-canvas detail panel into view. Web typecheck + esbuild bundle clean. (2026-06-25)
- **Web-native UI redesign — Phase 1 shell (PLANwebnativeui.md):** first slice of the web-native UI rebuild. Core gains one purely-additive batch intent `Intent::SetCursor(Path)` (a pointer analogue of the navigation intents) routed in `dispatch.rs` to a new thin `Session::set_cursor(path)` that places the cursor on a visible row by path (no-op off-tree). The Web UI replaces the monospace `<pre>` tree with a **web-native** `<div id="tree">` of clickable row `<div>`s; tree rendering is extracted to a new `web/render.ts` (pure `SessionSnapshot → DOM`, lifting `valueTypeClass`/`renderValue`/`isExpanded`/`escapeHtml` out of `ui.ts`). Visual redesign away from the terminal look: system-UI font for chrome with monospace reserved for values, per-depth indentation guide lines, an animated disclosure chevron (rotates on expand), row hover/cursor affordances (left accent bar on the cursor row), and key/value rendered as distinct styled spans (no `key = value` literal). Comments render in a dedicated muted-italic style. `web/ui.ts` slims toward an orchestrator and adds pointer wiring: clicking a leaf row → `SetCursor`; clicking anywhere on a branch row → `SetCursor` + `ToggleExpand`. New always-visible search-bar chrome (inert until Phase 4). Keyboard accelerators stay fully functional. The TUI is unchanged. New tests: 1 headless dispatch test + 1 serde-roundtrip variant + 2 functional-smoke checks; `cargo test`/clippy/fmt clean, `wasm-pack build` + web typecheck + esbuild bundle clean. (2026-06-23)
  - **Phase 1 completion — full chrome port (2026-06-24):** `index.html`/`style.css` are rebuilt from the `design_index_model.html` spec (presentation only — the mockup's fake JS model is discarded): the oklch-based `:root[data-theme]` token system with dark/light palettes, a redesigned toolbar (brand + format pill + dirty dot + Open/Save/Convert + undo/redo/theme icon buttons), a filter row (search box + type-filter + expand/collapse-all), a `.tree-wrap` with marquee/drop-line scaffolding, a sliding detail `<aside>`, footer with selection/clipboard pill badges, and the context-menu/kind/type-filter popovers + convert `<dialog>` markup (wired in later phases). `render.ts` now emits the design's **full** row anatomy keeping `data-path`/`data-index`: per-depth indent guides, drag grip, rotating SVG caret, key (or `—` for keyless array/AoT elements), branch item-count, `.eq`/`.val.t-{string,number,bool,date,null}` value coloring, a per-row **kind badge** (friendly kind label + scalar notation suffix + chevron), comment/trailing decoration, and hover action buttons (`＋` add on branches, `⋮` more). The badge/count need data `ViewRow` previously withheld (the TUI host computes its own `type_tag`), so core's `ViewRow` gains two purely-additive fields — `type_label: String` (`node_type_label_str` — `table`/`array`/`inline`/`array-of-tables`/`string`/… so the Web UI renders the real container kind, not a guess from `is_branch`) and `child_count: usize` (`Node.children.len()`) — populated in `visible_rows`; the TUI is unaffected. `ui.ts` is rewired to the new chrome IDs (toolbar/filter buttons → existing intents; Open uses the FS picker, falling back to the paste modal off-Chromium) and keeps a keyboard-driven `#overlay` fallback for modes not yet redesigned (Detail/Help/Prompt/Filter/KindSwitch/Convert/TypeFilter). render.ts also reconciles three core/design mismatches so the tree reads like the mockup: the synthetic root row (empty path) is not drawn and real top-level sections render flush-left (`depth − 1`); positional array/AoT elements (last path seg `Index`) render core's informative `[0]`/`[1]` index label faintly (vs the mockup's bare `—`); and comments are detected by `type_label === "comment"` (core fills a comment's `key` *and* `value` with the text, so the old key/value heuristic mis-rendered a leading comment as a `—` = `# …` leaf with a `[comment]` badge). **Row-click fix:** `data-path` carries JSON (`[{"Key":…}]`) but `escapeHtml` left `"` intact, truncating the attribute at the first quote so `JSON.parse(dataset.path)` threw and every row click/caret-toggle silently aborted — a new `escapeAttr` also encodes `"` (`&quot;`), so click-to-focus and branch expand/collapse work. The grip renders but its drag-reparent handler lands in Phase 2. No new intents this slice; +3 functional-smoke checks (type_label/child_count wire) + 2 serde-roundtrip fields; `cargo test -p confy-core` (438) / clippy / fmt clean, `wasm-pack build` + esbuild + `tsc --noEmit` clean.
  - **Phase 1 click wiring + `CommitEdit`/`CommitKind` (2026-06-24):** every visible row affordance is now functional (was render-only), which pulls the plan's Phase-3 inline-edit/kind intents forward. Core gains two purely-additive batch intents: `CommitEdit { value: Option<String>, name: Option<String> }` → `Session::commit_edit` (seeds a fresh `Mode::Edit` from the cursor via `begin_inline_edit`, overwrites the value/name buffers — `None` = keep — then runs the full `edit_commit`, so type-change / collision / trailing-comment prompts still fire) and `CommitKind { path, target: KindTarget }` → `Session::commit_kind` (applies `Mutation::ConvertKind` directly — the pointer analogue of `OpenKindSwitch`+`KindSwitchCommit`, no popup dance). Web: clicking a **value** → inline `<input>` (seeded from `BeginEdit`'s buffer, committed on Enter/blur via `CommitEdit{value}`, Esc cancels) with native text entry bypassing the modal `EditChar` path; clicking a **key** → rename `<input>` (`CommitEdit{name}`); clicking the **kind badge** → a popover populated *only* from `session.kindOptions(path)` (disabled/skipped when empty) → `CommitKind`; **＋** → `AddNode`; **⋮** and **right-click** → a context menu (Edit/Add/Copy/Cut/Paste/Delete/Remark/Detail/Undo/Redo, enablement from snapshot state — Paste only when `clipboard_count`); popovers close on outside-click/Esc. A `data-path`-attribute correctness audit confirms every `onTreeClick` selector (`data-caret`/`data-kind`/`data-act`/`data-edit`/`data-editing`) matches an attribute `render.ts` emits. New tests: 3 headless dispatch tests (`commit_edit` value-replace + key-rename, `commit_kind` int→hex) + 3 serde-roundtrip variants + 4 functional-smoke checks (CommitEdit value/rename, kind_options offers IntHex, CommitKind→hex) all through the wasm channel; full suite green (core 441 incl. new + tui 167), clippy/fmt clean, `wasm-pack build` + esbuild + `tsc` clean. Positional elements now show the informative `[0]`/`[1]` (user choice over the mockup's `—`). **Review checkpoint — manual web click-test next.**
  - **Phase 1 CSS re-base on `design_index_model.html` (2026-06-24):** to eliminate any visual drift, `web/style.css` is now the design's `<style>` block **verbatim** (the previous file was a Prettier-reformatted near-copy that under-specified the design's own `.cell-input`/`.key-input` inline-edit styles) plus a small fenced *app-only utilities* appendix the mockup has no equivalent for — `#overlay` keyboard fallback, `.modal`/`.modal-box` (external-edit + open modals), and `.hidden`/`.mono`/`.kind-note`/`.key.elem` (the mockup uses `.hide` / inline `var(--mono)` / inline `opacity`). A class-coverage audit over a real rendered tree confirms every class the rendered rows emit is defined in the CSS (no unstyled drift). UI labels stay **English** (the design is Chinese; i18n is a later option): `index.html` keeps its English chrome and gains the design's resting footer (richer keyboard hint + always-visible `none selected` / `clipboard 0` badges), `renderFooter` updates badge **text** rather than toggling visibility, and the branch item-count reads `N items`. No core/Rust changes; `tsc --noEmit` + esbuild + `wasm-pack` + `functional_smoke` (all) clean. **Review checkpoint stands — manual web test next.**
  - **Phase 2 — selection + drag-reparent + detail panel, plus two click-test bug fixes (2026-06-24):** Core gains two purely-additive pointer intents. `Intent::SetSelection { paths }` → `Session::set_selection` replaces the whole selection from a resolved set (drops non-visible paths, normalizes away selected descendants of selected ancestors, moves the cursor to the focal/last path); a new `Selection::set_all` folds the set into `committed`. `Intent::MoveSelectionTo { sources, target, index }` → `Session::move_selection_to` is drag-reparent, implemented as a one-shot cut→paste so it reuses `do_paste`'s entire `Mutation::Move` collision / illegal-destination / array-upgrade machinery; a drop onto a source or into its own subtree is rejected with the document untouched. New web modules: `web/select.ts` (pure click/⇧-range/⌘-toggle resolution + marquee hit-testing → `SetSelection`) and `web/dnd.ts` (HTML5 grip drag-reparent → `MoveSelectionTo`: drop onto a branch reparents into it, onto a leaf into the leaf's parent, with a `.row.drop-target` outline). `web/ui.ts` is wired up: a plain row-body click is now a **selection** gesture (expand stays on the caret, per the design); a marquee drag rubber-bands `#marquee` and selects intersecting rows (⇧/⌘/Ctrl unions); and the `Detail` mode now drives the design's slide-in `#detail` aside (close button → `ExitDetail`) instead of the keyboard `#overlay` fallback. **Two flaws found in the live click-test are fixed:** (1) *popups positioned at the top-left corner* — the kind-badge / ⋮ handlers called `SetCursor` (which rebuilds `tree.innerHTML`) **before** reading the anchor's `getBoundingClientRect()`, so the anchor was detached and returned all-zeros; the rect is now captured *before* the re-render (right-click already used `clientX/Y`, which is why only some popups were wrong); the orphaned `placePop`/`openKindMenu`/`openCtxMenu` helpers were removed. (2) *clicking a row didn't visibly do anything* — a plain click only moved the cursor bar because selection wasn't wired; it now selects. New tests: 4 headless dispatch tests (SetSelection replace+focal+drop-nonvisible, MoveSelectionTo reparent + self-subtree reject) + 2 serde-roundtrip variants + 7 functional-smoke checks, all through the wasm channel; full workspace suite green (core 442 incl. new, tui 167), clippy `-D warnings` / fmt clean, `wasm-pack build` + esbuild + `tsc --noEmit` clean. **Two fidelity fixes after the first click-test (vs `design_index_model.html`):** (1) *every node was flush-left with no level hint* — `render.ts` emitted one zero-width `.indent` span per depth level (CSS `.indent` carries no width), so nothing indented; it now emits a single spacer `width:calc(var(--indent) * level)` mirroring the design's `indent.style.width = depth*22`. (2) *the drop indicator was a uniform box outline* — `dnd.ts` is rebuilt to the design's drag model: a `dragover` computes `rel = (clientY − rowTop)/rowHeight`, and over a **branch**'s middle band (0.25–0.75) drops **into** it (append as child; `.drag-over-into` outline), otherwise drops **before/after** the hovered row as a sibling shown by the horizontal `#dropLine`. Sibling reorder is now supported (deferred no longer): the insertion index is read from the snapshot (an expanded parent shows all its direct children — comments included — in document order, so a child's visible position equals core's full-child-sequence index), and core's `Move` adjusts that original-sequence index for removed earlier siblings (verified: moving `a` to after `b` in `a,b,c` → `b,a,c`). The redundant `.row.drop-target` CSS is removed (the design's `.drag-over-into`/`.drag-src`/`.drop-line` were already present from the verbatim re-base). +1 headless reorder test (core 443). **Review checkpoint — re-test selection/marquee/drag-reparent/detail next.**

  - **Phase 4 — native search / type-filter / convert widgets (2026-06-24):** the three modes that still fell back to the keyboard-driven `#overlay` are now real web-native widgets. Core gains three purely-additive batch intents, each reusing existing machinery: `Intent::SetFilter(String)` → `Session::set_filter` sets the whole filter text at once and recomputes (non-empty → `FilterResults`, clearing drops to the resting mode, still `FilterResults` if a type filter is narrowing); `Intent::SetConvertFormat(DocFormat)` → `Session::set_convert_format` picks the convert target by value (a `<select>`) and reseeds the output path's extension (mirrors `convert_pick_format` minus the host stem); `Intent::SetConvertPath(String)` → `Session::set_convert_path` sets the whole output path at once (an `<input>`). All three are routed in `dispatch.rs`. **Web UI:** the always-visible search box is enabled and owns the filter text — an 80 ms-debounced `input` dispatches `SetFilter`, the clear button resets it, and `/` focuses it (no `Mode::Filter` is ever entered); the `f` type-filter renders into the native `#tfPop` popover (`menu-label` groups + `data-state` `.tf-cell` buttons with the design's check-`.box`, a tri-state Partial style added), where clicking a cell dispatches a `TypeFilterMove` delta + `TypeFilterToggle` and Apply/Cancel commit/exit; convert opens the native `#convDlg` `<dialog>` (format `<select>` → `SetConvertFormat`, output-path `<input>` → `SetConvertPath`, a warnings list, and a Convert/Confirm button that runs `ConvertRun`→`ConvertConfirm`). The `#overlay` fallback now serves only Help / Prompt / `K` kind-switch; the dead `Filter`-mode keyboard branch and the orphaned `#overlay .tf-*` CSS are removed, and the body-keydown accelerator guard now skips `INPUT`/`TEXTAREA`/`SELECT` so typing in the search box / dialog doesn't trigger navigation. New tests: 3 headless dispatch tests (SetFilter narrow+clear, SetConvertFormat seeds path, SetConvertPath→ConvertRun writes) + 3 serde-roundtrip variants + 4 functional-smoke checks; full suite green (core 438 lib + 38 headless + 11 serde-roundtrip, tui 167), clippy `-D warnings` / fmt clean, `wasm-pack build` + esbuild + `tsc --noEmit` clean. **Review checkpoint — re-test live search / type-filter grid / convert-with-warnings → save next.**
  - **Phase 4 follow-up — Batch 1 bug fixes (2026-06-24):** seven fixes from live browser testing. (#1) **Search now matches values, not just keys** — `Session::recompute_filter` passed `None` for the leaf value to `haystack`, so a search only ever hit keys/paths/comments; it now feeds a scalar's value into the haystack (this reverses the old "value never matched" rule for **both** the Web UI and the TUI). (#2) **Kind-badge notation** — a default basic string now shows its notation suffix (`str·"…"`, via a new `BasicString` entry in `render.ts`'s `NOTATION_SHORT`), and a multiline value's newlines are collapsed to `↵` in the display so the value cell stays one line and never pushes the kind badge off the row (it was becoming unclickable after a switch to a multiline notation). (#4) **Type-filter popover** — `#tfPop` gains `max-height`/`overflow-y:auto`/`max-width` and roomier cell spacing so a tall facet list scrolls instead of overflowing off-screen and the cells stop cramming. (#5) **Convert "failure" UX** — a lossy convert is non-fatal (warnings + a second confirm writes); the warnings panel is reworded to "Lossy conversion — these styles will be normalized; the output is still valid" and recolored amber (was the red error palette), so it no longer reads as a failure. (#6) **Cursor / selection no longer decouple** — plain `j/k/g/G/↑/↓/Home/End` now collapse the selection onto the new cursor row (via `SetSelection`), so the selected highlight — and what `d/c/x` act on — follows the cursor bar; `⇧+↑/↓` still extends the multi-select range, and paste-mode slot nav is left untouched. (#7) **`Delete` key** is bound to delete (same as `d`). (#8) **Space toggles the detail side panel** (`ToggleDetail`) while `Enter` keeps expand/activate; `Esc` already closes the panel. Web-only except #1; new headless + functional-smoke checks for value-search (core 439 headless suite, smoke 19 checks), clippy `-D warnings` / fmt clean, `wasm-pack build` + esbuild + `tsc --noEmit` clean. **Review checkpoint — re-test the seven fixes in the browser before Batch 2 (enhancements #3/#9/#10/#11).**
  - **Phase 4 follow-up — Batch 1.5 fixes (2026-06-24):** six web-only refinements from a second browser test (no core/Rust change). (1) **Complete scalar notation** — `render.ts`'s notation suffix was missing the *default* styles, so integer `Decimal`/plain `Float` showed a bare `int`/`float`; the suffix is now type-aware (`·dec` for decimal ints and plain floats, alongside `·"…"`/`·'…'`/`·0x`/`·1e`/…), so every multi-style scalar shows its writing style and single-style scalars (bool/datetime/null) stay bare (the type label is complete). (2) **Kind popup reopens immediately** — `closePops()` only dropped the `.open` class but left the `document` outside-click listener registered, and each open added another; on a reopening click the stale listener fired later in the same click and flashed the menu shut (hence "click elsewhere first, then back"). There is now a single shared closer that `closePops()` removes, and `placePopAt` closes/clears it **synchronously** before opening. (3) **Popups no longer share state** — the mode-driven type-filter `#tfPop` and the click-driven `#kindMenu`/`#ctxMenu` all carried class `.pop`, so `closePops()`/the Escape handler acted on all of them and the type-filter popover opened/closed together with a kind menu; `closePops` is now scoped to the two click-menus and `#tfPop` is left entirely to `renderTypeFilterPop`. (4) **Segmented shift-range selection** — `select.ts` shift-click discarded any prior selection; it now unions the `anchor…clicked` range onto a `base` snapshot captured when the anchor is set (plain click → empty base; **⌘/Ctrl-click adds an anchor without clearing**, base = the toggled set; marquee → base = its result), so select 1–3, ⌘-click 5, shift-click 7 → `1–3,5–7`, and re-shift-clicking redefines (can shrink) the range from the anchor. (5) **Visible paste destination** — in paste mode the clipboard freezes the selection, so a row click now moves the **cursor** (= `After(cursor)` paste target) via `SetCursor` instead of a frozen `SetSelection`, and a new `body.paste-mode` class renders the cursor row as an unmistakable "▸ paste here" target (it was invisible before, though pasting did work). (6) **Keyboard survives a button click** — the body-keydown focus guard dropped `"BUTTON"` (keeps `INPUT`/`TEXTAREA`/`SELECT`), so shortcuts keep working after clicking a toolbar/row button. `tsc --noEmit` + esbuild + `functional_smoke` (19) clean; no Rust change so no wasm rebuild needed. **Review checkpoint — re-test the six in the browser, then Batch 2.**
  - **Phase 4 follow-up — Batch 2 enhancements (2026-06-24):** two web-only additions (no core/Rust change). (#3) **Container kind notation** — the kind badge now shows a notation suffix on *containers* too (a new `CONTAINER_NOTE` map in `render.ts`): `table·scope`/`table·dotted` (TOML standard vs dotted-key table), `array·inline`/`array·multi`, and YAML `table·block`/`table·flow`/`array·block`/`array·flow`; a suffix that would just repeat the label (an inline table is already labelled `inline`) is suppressed, mirroring the TUI's `[T/S]`/`[T/D]`/`[A/M]` distinctions which the badge previously dropped for branches. (#9) **Per-format kind help** — the `?` Help overlay now appends a backend-specific KIND legend (`KIND_LEGEND` in `ui.ts`, keyed by `doc_format`, ported from the TUI's `TOML_HELP`/`JSON_HELP`/`YAML_HELP`), explaining each container/scalar label·notation for the open file's format (TOML radix/dotted/AoT, JSON null/exponent, YAML block/flow/opaque/string-styles). (#10/#11) **File-open + filename** verified already wired (FS-API `pickOpenFile`→`showOpenFilePicker` with paste/download fallback; filename shown in the title) — no change. `tsc --noEmit` + esbuild clean; no wasm rebuild needed. **Review checkpoint — re-test the kind badges + Help legend, then Batch 3 (read-only raw tab).**
  - **Phase 4 follow-up — Batch 3: read-only Raw view (2026-06-24, #12):** a `Tree | Raw` segmented toggle (new `#viewTabs` in the filter bar) flips the main pane between the interactive tree and a **read-only** `<pre id="raw">` of `session.serialize()`. The Session stays the single source of truth — Raw renders the live serialized document (including unsaved edits) on every `render()`, so it never drifts; the tree DOM is kept (hidden) so toggling back is instant. Web-only: `index.html` (toggle + `#raw` pane), `ui.ts` (`rawView` flag, `setView`, `renderRawOrTree` hooked into `render`, two button bindings), `style.css` appendix (`.viewtab.active` pressed-segment + `.raw-view` scrollable text). Per the agreed scope (read-only first), there is no in-Raw editing and thus no save-time format guard yet — Save still serializes from the Session, which is always valid; an editable Raw tab + format guard is a later, separate step. `tsc --noEmit` + esbuild clean; no wasm rebuild. **Review checkpoint — toggle Tree/Raw across TOML/JSON/YAML, edit in Tree and confirm Raw reflects it, then this closes the post-Phase-4 bugfix/enhancement batches.**
  - **Phase 5 — documentation sync (2026-06-24):** `WEBUI.md` and the `CLAUDE.md` `web/` module map are brought up to date with the web-native redesign (they predated it, describing the old keyboard-driven monospace UI). `WEBUI.md`: architecture diagram + intro now describe the pointer-first port of `design_index_model.html` (Session as single source of truth, `style.css` verbatim + appendix); the data-model section documents the additive **batch intents** (`SetCursor`/`SetSelection`/`MoveSelectionTo`/`CommitEdit`/`CommitKind`/`SetFilter`/`SetConvertFormat`/`SetConvertPath`) and `ViewRow`'s new `type_label`/`child_count`; the Web-UI-architecture section is rewritten around `render.ts` row anatomy, `select.ts` pointer/marquee selection, `dnd.ts` drag-reparent, inline-edit/kind/context popovers, the native search/type-filter/convert widgets that replaced the keyboard `#overlay` (now Help/Prompt/KindSwitch only), the Tree|Raw read-only view, paste-mode cursor target, value search, and the per-format Help legend. `CLAUDE.md`: the `web/` map adds `render.ts`/`select.ts`/`dnd.ts` and notes the verbatim-CSS convention + Tree/Raw toggle. Docs-only; no code change. (The "accelerator polish" half of Phase 5 is deferred to the user's browser-test findings rather than invented.)
  - **Phase 5 follow-up — Batch 4: eight browser-test fixes (2026-06-24):** the accelerator-polish pass, driven by live findings. Core (additive only): `SessionSnapshot` gains `clipboard_cut: bool` + `clipboard_paths: Vec<Path>` (from `Clipboard.cut`/`.sources`) so the UI can mark clipboard source rows; and `Session::escape` now discards a pending async external edit (it lives outside `Mode`) before anything else — without this the snapshot's `external_edit` stayed set and the host reopened the multi-line modal forever (the **"Cancel does nothing"** bug, #6). Web: (#1) the kind badge **toggles** its popup — a second click on the same badge closes it (tracked via `kindMenuPath`); (#2) clipboard source rows get a distinct dashed ring — **copy** = accent, **cut** = amber + dimmed + strike — clearly apart from the filled selection box; (#3) the kind popup prepends a disabled **"Current: label·notation"** header (design's `目前：…`, via a new `currentKindLabel` in `render.ts`); (#4) the search box shows its `×` clear button only when it has text (`.search.has-val`) and **Esc** clears the query when present, else drops focus back to the tree; (#5) the type-filter is now toggle-and-live — the toolbar button opens the popup or closes it keeping the filter (`CommitTypeFilter`), the Apply/Cancel foot is replaced by a header `×` that clears the filter *and* closes the popup (`ExitTypeFilter`), Esc does the same, toggling a facet cell keeps the popup open, and only a press *outside* the popup closes it keeping the filter (`CommitTypeFilter`, detected on `mousedown` so a cell toggle's re-render doesn't orphan the target); (#7) **Open** uses a native `<input type=file>` fallback (works in every browser) instead of the paste modal when the FS Access API is absent, reading `file.name`/`file.text()`; (#8) the filename now shows in the toolbar title in those browsers too (the fallback previously opened without a name). New tests: 2 headless dispatch tests (escape cancels pending external edit; cut flag + ExitTypeFilter closes the popup) + the clipboard-copy test now asserts `clipboard_cut`/`clipboard_paths`; full suite green (core 441 incl. new, tui 167), clippy `-D warnings` / fmt clean, `wasm-pack build` + `functional_smoke` + esbuild + `tsc --noEmit` clean. **Review checkpoint — re-test the eight fixes in the browser.**

  - **Phase 5 follow-up — narrow-width chrome + kind-popup polish (2026-06-24):** two browser-test refinements, web-only (no core/wasm change). (1) **Filename survives narrow widths** — `#title` previously reused the design's generic `.label-hide` class, which `display:none`'d it below 920px; since we repurposed it to show the real filename, the name vanished exactly when still needed. It now carries a dedicated `.doc-name` class that truncates gracefully (`max-width` + ellipsis) and exposes the full name via the `title` tooltip on hover. (2) **Footer warnings read in full** — a long `.status.err` was clipped by `nowrap`+ellipsis competing with the hint/badges; the footer now `flex-wrap`s and the error takes its own full-width line and wraps (`flex-basis:100%`, `white-space:normal`). (3) **Kind popup matches the design** — the "Current:" header and every option button gain a leading right-chevron `>` icon (the design's `IC.caret`, now exported from `render.ts`), via the existing `.menu-item .ic` style. `tsc --noEmit` + esbuild clean.

  - **Phase 5 follow-up — footer space, warning emphasis, copy-row distinction (2026-06-24):** three more browser-test refinements, web-only. (1) The static pointer hint (`click select · ⇧click range · …`) is removed from the footer and folded into the Help overlay (`?`) as a new **"pointer"** section — freeing footer width for status/warnings. (2) The footer **warning/error now stands out**: rendered as a tinted pill in the warning hue (`--t-bool`, `font-weight:600`, soft background + border) on its own full-width line. (3) The **copied** source row no longer mimics the selection box — it now uses a **dashed green ring** (`--t-string`, `outline` dashed) instead of a solid accent ring that read identically to the selection edge; **cut** switches to a matching dashed purple ring (`--t-date`) + dim + strike, so selected / copied / cut are three clearly distinct states. `tsc --noEmit` + esbuild clean.

  - **Phase 5 follow-up — edit-key leak, branch popup editing, button tooltips (2026-06-24):** three more browser-test fixes. (1) Pressing **`e`** (and **`a`**) no longer leaks the triggering character into the editor — the tree key handler now `preventDefault()`s these editor-opening keys before the new inline `<input>` / external `<textarea>` is focused. (2) **Every branch now opens the popup editor.** Core (web-only): `dispatch()`'s `BeginEdit` arm routes all container nodes (Table / InlineTable / Array / ArrayOfTables) to the external modal instead of inline — a branch row has no value cell, so a single-line inline table/array previously routed to `EditKind::Inline` and rendered no editor (the **"inline table = no response"** bug); multiline containers were already external. The TUI is unaffected (it calls `edit_target_kind`/`begin_inline_edit` directly, not `dispatch`), so `BEHAVIOR_MATRIX §6` inline-table editing there is unchanged. (3) The `Open` / `Save` / `Convert` / `Type filter` toolbar buttons gained `title` **tooltips** (their text labels hide at narrow widths). New test: `dispatch_edit_inline_table_routes_external`. Full suite green (core 442 incl. new), clippy `-D warnings` / fmt clean, `wasm-pack build` + `functional_smoke` + `tsc --noEmit` + esbuild clean. (4) The external popup editor now closes on **Esc** (same as Cancel — peels the pending edit; Enter stays free for newlines), via a `keydown` handler on the textarea.

  - **Phase 5 follow-up — scalar value colors + click-to-clear selection (2026-06-24):** two more browser-test fixes, web-only. (1) **Boolean and datetime values are now color-coded.** `types.ts`/`render.ts` checked for `"Boolean"`/`"Datetime"`, but the serde wire names from `model::node::ScalarType` are `Bool` and `OffsetDatetime`/`LocalDatetime`/`LocalDate`/`LocalTime` — so those never matched and fell through to the default (uncolored). The `ScalarType` mirror and `valueTypeClass` switch now use the real variant names, so bools (`--t-bool`) and all four datetime kinds (`--t-date`) get their design colors like strings/numbers/null already did. (2) **Clicking empty tree space clears the multi-select** (`SetSelection { paths: [] }`; cursor stays put, no-op during paste mode and when nothing is selected). `tsc --noEmit` + esbuild clean.

  - **Phase 5 follow-up — comment-node display + inline edit (2026-06-24):** two more browser-test fixes, web-only (`render.ts`, `style.css`). (1) A **multi-line comment** now shows only its **first line** in the tree row (with a trailing `…` + tooltip when it continues) instead of the newlines collapsing into a run-on line; the full text remains in the detail panel (`i`). (2) **Single-line comment nodes now edit inline.** Core already routed a single-line comment to `EditKind::Inline` and committed it via `EditComment`, but `render.ts` never emitted an `<input>` for a comment row, so the inline edit showed nothing (the inline-table-style gap); a comment row in `Value` edit mode now renders an inline `<input data-editing="comment">` reusing the existing `focusInlineEdit`/`CommitEdit` path — no core/wasm change. Multi-line comments still route to the popup editor. `tsc --noEmit` + esbuild clean.

  - **Phase 5 follow-up — sample doc + format pill, consistent add-child, menu clamp (2026-06-25):** five browser-test refinements. (1) **The startup demo doc is now named "sample"** (was "confy") and ships one variant per backend (`SAMPLES` map in `ui.ts`). (2) **The header format pill toggles formats while in sample mode** — clicking it cycles TOML → JSON → YAML, reloading the matching sample; opening or saving a real file leaves sample mode and freezes the pill as a static label (`sampleMode` flag, `.fmt-pill.toggleable` affordance). (3) **`＋` and the menu's "Add child" always add a child** regardless of the branch's open/closed state — the TUI couples child-vs-sibling to expand state, but the pointer UI is now consistent. Core gains two **explicit, additive** intents `AddChild`/`AddSibling` (`add_child`/`add_sibling` force `is_append` true/false; the existing `add_node`/`a`-key keep the expand-based TUI behavior), so the web no longer relies on expand side-effects. (4) The node menu gains an **"Append sibling"** item (enabled for any non-root node) so both insert modes are explicit. (5) **The context/kind popup never spills off-screen** — `placePopAt` now measures the menu's height and clamps `top` so a menu opened near the bottom slides up to stay fully visible (was clamped to a fixed `innerHeight-40` that ignored menu height). New tests: 2 headless dispatch tests (`AddChild` nests into a collapsed branch; `AddSibling` stays a root sibling) + 2 functional-smoke checks; full suite green (core 444, tui 167), clippy `-D warnings` / fmt clean, `wasm-pack build` + `functional_smoke` (21) + `tsc --noEmit` + esbuild clean. **Review checkpoint — re-test the five in the browser.**
  - **Phase 5 follow-up 2 — self-describing sample + paste retargets selection (2026-06-25):** three more refinements. (1) **The sample doc now carries identical data across all three formats** — replaced the three ad-hoc per-backend `SAMPLES` with one shared, self-describing "intro to confy" tree (`about`/`basics`/`formats`/`fun` sections, leading welcome comments, a trailing comment, arrays, mixed scalar types) rendered in each dialect's native notation; a throwaway core check confirmed the projected node trees are byte-for-byte structurally identical (only the comment marker `#`↔`//` and scalar quoting differ). (2) **The content is now an introduction to confy** — what it is, basic pointer/keyboard operations, the three dialects, and a `fun` section, with light humor + emoji. (3) **Copy → paste now retargets the selection onto the pasted node** — `do_paste`'s success path drops the source (copied/cut) selection and sets both the cursor and the selection to the freshly-pasted node(s), which land contiguously from `target.index` in the destination parent's rebuilt child sequence (universal — TUI + Web, per the shared `do_paste`). New tests: 1 headless dispatch test (`dispatch_paste_retargets_selection_to_pasted_node`: copy `t1.x`, paste after `t2.y` → cursor + sole selection on `t2.x`) + 2 functional-smoke checks; full suite green (core 445, tui 167), clippy `-D warnings` / fmt clean, `wasm-pack build` + `functional_smoke` (23) + `tsc --noEmit` + esbuild clean. **Review checkpoint — re-test the sample/format-cycle + copy-paste selection in the browser.**
  - **Phase 5 follow-up 3 — separate inline-comment editing, branch trailing comments, create-on-missing (2026-06-25):** four items. (①) **The web node menu drops Undo/Redo** — they remain on the toolbar buttons and `z`/`y`, so the per-node context menu is no longer cluttered with document-wide actions. (②) **The web edits a node's trailing inline comment separately from its value** (the TUI still bundles `value␠␠# comment` in one buffer, which is fine there). Core gains one additive intent `Intent::SetTrailing { path, comment: Option<String> }` → `Session::set_trailing_comment` (wrapping the existing `Mutation::SetTrailingComment`, atomic + semantically validated). Web: the value `<input>` is seeded with the **value only** (`render.ts` strips the bundled trailing suffix) and re-attaches the unchanged comment on commit so a value edit never drops it; clicking the trailing-comment cell opens its **own** small web-local `<input>` → `SetTrailing` (empty clears). (③) **The node menu gains "Append comment"** (after "Toggle comment"), offered on any non-comment row without an existing trailing comment — **including branches** — opening the same separate comment editor. (④) **Branch nodes can now carry a trailing inline comment in all three backends.** TOML projection captures a `[section]  # c` / `[[aot]]  # c` header's EOL comment onto the table/AoT-entry node, and `set_trailing_comment` was extended to splice after the header's `]`/`]]`; YAML projection already surfaced a block-map parent's `key:  # c` (display) and the splice was extended to write the comment on the `key:` line for block-collection values (a block *scalar* `|`/`>` still rejects, as does a block value reached through a seq element); JSON already supported object/array members through the existing member-trailing path (verified + test-locked). The **TUI** now **displays** a branch's trailing comment in the VALUE column (leads the cell with no separator, since a branch has no value preview). (④′ for the TUI) **`confy <file>` creates the file when it doesn't exist** — it prompts `Create <path> as <FORMAT>? [y/N]` (format from the extension), writes a minimal valid seed (`seed_for`: TOML/YAML empty, JSON `{}`), and opens it normally; a non-interactive stdin aborts cleanly without touching disk. New tests: TOML header + JSON object-branch + YAML block-map-parent `set_trailing_comment` tests (the old YAML "rejects block value" test flips to assert success for collections and keeps a block-*scalar* rejection), 1 headless `dispatch(SetTrailing)` test (scalar + branch), `seed_for` round-trip test, +3 functional-smoke checks; full suite green (core 467 + tui 162 + ffi/web, 686 workspace), clippy `-D warnings` / fmt clean, `wasm-pack build` + `functional_smoke` (26) + `tsc --noEmit` + esbuild clean. **Review checkpoint — re-test in the browser: separate value/comment editing, Append comment on leaves + branches, branch comment display; and in the terminal: `confy newfile.toml` create-on-missing.**

- **Web UI: save-in-place + polish (PORTING.md §8 follow-up):** the Web UI gains real file I/O, a dark/light theme, and richer state. (1) **Save-in-place** via the File System Access API (`web/fs.ts`): `Ctrl-o`/Open opens a real file (`showOpenFilePicker`) and `Save` writes in place to the held `FileSystemFileHandle` (or Save-As on first save), with a download fallback for Firefox/Safari and the Load/paste modal always available. The "Open…" button auto-hides on browsers without the API. Convert output routes through Save-As/download too. Core `Intent::Save` is unchanged (still just clears the dirty flag) — all I/O stays host-owned behind `web/fs.ts`; non-browser hosts are unaffected. (2) **Dark/light theme toggle** (titlebar `☾`/`☀`, CSS-variable palettes, persisted in `localStorage`). (3) **Multi-select + paste UX**: selected rows render with an `◉` marker and tint (selection count in the footer); a new `SessionSnapshot.clipboard_count: Option<usize>` exposes live clipboard state as structured data (not derived from status strings) — the footer shows "clipboard: N" after copy/cut. (4) **Full type-filter facet grid**: `ModeView::TypeFilter` is now a struct variant carrying a `TypeFilterView { rows, cursor_row, cursor_col, active }` projected from the authoritative `session/type_filter::layout` (+ tri-state `CheckState` now serde-derived), so the UI renders the per-format facet grid with `[✓]/[~]/[ ]` glyphs + cursor highlight without duplicating layout logic; the host never re-derives the facet set. (5) **Visual refinement**: scalar values are color-coded by `scalar_type`. New `TypeFilterView`/`TypeFilterRow`/`TypeFilterCellView` view types; `CheckState` serde-derived. New tests: 2 headless dispatch tests (type-filter grid projection + clipboard_count) + 11 functional-smoke checks. Full suite: 662 tests pass, clippy/fmt clean; `wasm-pack build --target web` succeeds; web typecheck clean; 36/36 functional smoke checks pass. Structured row diff (§8.3 G2) remains deferred. (2026-06-18)

- **Stage 2 — WASM FFI + Web UI (PORTING.md §8):** a third workspace crate `confy-ffi` wraps `confy-core` for WebAssembly (`wasm-bindgen` + `serde-wasm-bindgen`), exposing one JS-facing handle `ConfySession` with a single command channel `dispatch(intent) -> SessionSnapshot`. The new `Session::dispatch(Intent) -> SessionSnapshot` (in `crates/confy-core/src/session/dispatch.rs`) mirrors the TUI event loop's mode-dependent routing as a direct Intent→method map and is the only command channel the Web UI uses (independently unit-tested headlessly). `Session::snapshot()` returns the full renderable state. **§8 design decisions resolved:** (1) rich serde — `Node`/`NodeKind`/`NodeTree`/`KeySign`/`DocFormat`/`ConvertStep`/`EditField`/`EditKind`/`FilterLayer` now derive `Serialize`/`Deserialize` alongside the Phase-E leaf types; (2) async host — WASM does **not** route through the sync `Host` trait; instead `dispatch` returns an `external_edit` signal in the snapshot and the JS host opens its own async modal, then re-dispatches `ApplyReplace`/`ApplyEditComment` (Session remembers the resolution in a new `pending_external_edit` field + `PendingExternalEdit` type); (3) full-state transport — the snapshot carries the entire visible tree + a `ModeView` projection of mode/modal surfaces (`EditView`/`ConvertView`/`KindOptionView`/`PromptView`/`ExternalEdit`/`ExternalEditKind`), no structured row diff. `Intent` variants `BeginInlineEdit`/`BeginInlineRename` renamed to `BeginEdit`/`BeginRename` for contract clarity. New `web/` directory holds the TypeScript integration (`types.ts` contract mirror, `confy.ts` typed wrapper) and a minimal functional Web UI (`index.html`/`ui.ts`/`style.css` + `build.mjs`/`serve.mjs`, esbuild-bundled). New `WEBUI.md` documents the FFI boundary and UI architecture. `crates/confy-ffi/functional_smoke.mjs` is a 25-check node verification of the full Intent→snapshot contract (load, navigate, nudge, inline edit, multiline external-edit handshake, undo/redo, save, quit-prompt) across TOML/JSON. wasm32-unknown-unknown builds (`wasm-pack build --target web`); `getrandom` `wasm_js` enabled for the ahash-via-taplo chain. The TUI is unchanged. Full suite: 660 tests pass, clippy/fmt clean. (2026-06-18)
- **Serde + fake-Host tests, slice 5 Phase E (PORTING.md §5 Phase E, §7 exit gates #3 and #5):** `Intent`, `ViewRow`, `Update`, and `Mutation` (plus the leaf types they reference — `Seg`, `ScalarType`, `Format`, `KindTarget`, `Target`, `OnCollision`) now derive `Serialize`/`Deserialize` via a new `serde` workspace dependency on `confy-core` (unconditional; `serde_json` is a `confy-core` dev-dependency only — no runtime cost in the TUI binary). New `crates/confy-core/tests/serde_roundtrip.rs` (gate #3) asserts each type survives a `serde_json` serialize→deserialize round-trip (compared as `serde_json::Value`, so no `PartialEq` had to be added to the domain types). New fake-`Host` `$EDITOR` tests in `session_headless.rs` (gate #5) drive the multi-line external-edit flow headlessly — a `FakeHost` impl of the `Host` trait returns canned edited text, and the test composes `edit_target_kind`/`external_edit_path`/`serialize_fragment`/`Host::edit_text`/`apply_replace` (no real editor spawn, no terminal) to assert the edited text lands in the doc and that a cancelled edit leaves it untouched. This rehearses the WASM/JS-interop contract and proves the `$EDITOR` path is host-agnostic. Full suite: 438 core-unit + 167 tui + 26 integration + 15 session-headless + 5 serde-roundtrip; clippy/fmt clean. (2026-06-18)
- **Thin App wrapper, slice 5 Phase D (PORTING.md §5 Phase D):** `App` is rewritten as a thin Host wrapper: it holds `pub session: Session` (all CORE state) plus five HOST-only fields (`rows: Vec<RowSnapshot>`, `source_path`, `detail_scroll`, `help_scroll`, `table_offset`). Every CORE operation method is now a 1-line delegate to `self.session.*`. `RowSnapshot` (the HOST view model for ratatui) adds `type_label`/`type_tag`/`scalar_type` on top of `ViewRow`. `rebuild_rows()` calls `session.compute_rows()` then maps `ViewRow→RowSnapshot` by looking up `NodeKind` for the TYPE column tag. HOST-split methods (`edit_node`, `save`, `convert_write`) remain on `App` and do all filesystem I/O. `mod.rs` and `ui.rs` field accesses updated (`app.X` → `app.session.X` for all CORE fields). `selection.clear()` removed from `compute_rows()` — selection is path-keyed (Slice 3) and survives structural changes; the old `rebuild_clears_stale_selection` test updated to assert the correct path-keyed invariant. `#[cfg(test)]` attributes added on methods only needed by tests. Full suite: 438 core-unit + 167 tui + 26 integration + 13 session-headless; clippy/fmt clean. (2026-06-17)
- **Workspace split (PORTING.md slice 1):** the single `confy` crate is now a Cargo workspace of `confy-core` (the headless model — `crates/confy-core/`, pure: no terminal/UI/`tempfile` runtime deps) and `confy-tui` (the ratatui TUI + CLI, `crates/confy-tui/`, which depends on `confy-core` and `pub use confy_core::model` so its UI modules keep their `crate::model::…` paths). The binary is still named `confy`. Dependencies moved to `[workspace.dependencies]`; `clap`/`ratatui`/`crossterm`/`fuzzy-matcher`/`dirs` are now `confy-tui`-only; `tempfile` is a `confy-core` dev-dependency only. Integration tests split by crate (`roundtrip*`/`yaml_scratch` → `confy-core`, `convert_cli` → `confy-tui`); fixtures moved under `crates/confy-core/tests/`. One `pub(crate)` model helper (`cst_edit::joinable_entry`) widened to `pub` for the cross-crate TUI paste-forming pre-check. No user-visible behavior change. (2026-06-17)
- **FS boundary, slice 1 (PORTING.md §2 A1+A3):** each backend gained a file-system-free `from_str(text)` constructor (and `AnyDocument::from_str_as(text, format)`); `load` is now `fs::read` + `from_str` + source `path`/`filename`. The document-conversion **reparse safety-net** now re-parses the rendered string in memory via `from_str_as` instead of writing a `NamedTempFile` and re-`load`-ing it — removing the only runtime `std::fs`/`tempfile` use in the model's conversion path. (Severing `load`/`save` file I/O fully from the core and dropping the `path` field — §2 A2/A4/A5 — is the next slice.) (2026-06-17)
- **FS boundary, slice 2 (PORTING.md §2 A2/A4/A5 + §7 gate):** `confy-core` is now **completely filesystem-free at runtime**. Removed `ConfigDocument::load`, every backend's `load`/`save`, and `AnyDocument::load_as`/`save`; dropped the `path: PathBuf` field from all three backends (`filename` stays as a host-set display label via the new `set_filename`). `from_str`/`from_str_as` are the sole constructors. The host owns all I/O: a new `confy_tui::load_document(path, format)` reads the bytes, parses via `from_str_as`, sets the path-derived label, and enables JSONC comments for a `.jsonc` extension; `App::save` now `serialize()`s and `std::fs::write`s to `App::source_path`. `detect_format` (pure extension match) stays in core. A §7 boundary gate (`crates/confy-core/tests/no_fs_gate.rs`) scans the core's runtime code and fails on `std::fs`/`std::process`/`std::env`/`tempfile`/`crossterm`/`ratatui`; `tempfile` is no longer a `confy-core` dependency at all. ~30 call sites migrated (unit-test string constructors → `from_str`; integration tests → read-then-`from_str`). No user-visible behavior change. (2026-06-17)
- **State-machine lift, slice 4 (PORTING.md §5 Phases A–C):** `confy-core/session/` now contains the complete `Session` struct with all CORE fields (`doc`, `cursor`, `expanded`, `selection`, `mode`, `clipboard`, `paste_slot`, `filter*`, `type_filter`, `history`, `status`, `error`, `pending_edit`, `pending_trailing`, `detail_text`) and every CORE operation — navigation (`cursor_down/up/home/end`, `toggle_expand`, `collapse_all/expand_all`, `expand/collapse_level`), selection, filter/type-filter, kind-switch, convert orchestration, inline-edit, all mutations (add, delete, replace, rename, move, remark, cut/copy/paste, nudge), undo/redo, escape dispatch, and prompt dispatch. `Session::visible_rows() -> Vec<ViewRow>` is a pure on-demand view computation (no side effects). New types: `Intent` enum (all key-mapped actions a UI can dispatch), `Host` trait (`edit_text` callback for `$EDITOR`/multiline path), `Update` struct (what changed after a dispatch — `rows_dirty`, `status`, `error`, `quit`, `external_edit`, `convert_write`), `PendingCommit`, `EditKind`. Free helper functions (`node_type_label`, `format_label`) moved to session. The session is headlessly testable with no TUI or filesystem; `crates/confy-core/tests/session_headless.rs` exercises `Session` across TOML/JSON/YAML in 13 tests (§7 exit gate #4). `App.rs` is unchanged (Phase D thin-wrapper rewrite deferred to Slice 5). Full suite passes: 438 core-unit + 167 tui + 26 integration + 13 session-headless. (2026-06-17)
- **Identity reshape, slice 3 (PORTING.md §3):** the TUI cursor is now addressed by **node `Path`**, not a render-row `usize`. `App.cursor: Path`; `Selection` is re-keyed `HashSet<Path>` (its range-extend takes the ordered visible-path slice instead of a contiguous integer interval); `PasteSlot::{Into,After}` carry a `Path`. The navigation/selection/paste logic reads a new `App::visible_paths()` and `App::cursor_row()` rather than indexing `rows`; the **single** index↔path bridge is `App::cursor_row_index()`, used only for the ratatui highlight/viewport and the position footer. `insertion::resolve_target` now takes `(path, is_branch, expanded, sibling_index)` instead of a `&RowSnapshot`, so it no longer depends on the host render row. Cursor re-snap after a rebuild preserves the pre-reshape behavior (lands on the same visible position when the cursor's path disappears). Touched methods carry `§5: CORE/HOST/SPLIT` seam comments for the upcoming state-machine lift. No user-visible behavior change; the full suite (415 core-unit + 190 tui + 26 integration) passes, clippy/fmt clean. (2026-06-17)

### Docs
- `CLAUDE.md`: module map updated for the workspace split (two-crate tree, `from_str`/`from_str_as` on the backend/`AnyDocument` lines, per-crate `tests/` locations) and the closing note now records the file-system-free `from_str` primitive + the next-slice pointer. (2026-06-17)
- New `PORTING.md` at repo root: the design record for the Headless Core extraction and the multi-platform port (TUI + Tauri desktop + web + VSCode, sharing one Web UI compiled against the core via WASM). Captures the decisions taken (state machine lifts into Rust core; cursor/selection identified by `Path` not row index; one `Host` capability for the `$EDITOR`/multi-line path), the FS boundary to sever in `model/` (`load`→`from_str`, `save`→host, the `convert.rs` tempfile reparse-net, drop `self.path`), a per-file/per-function portability map (CORE ~70% / SPLIT ~20% / HOST ~10%) for `app.rs`/`selection.rs`/`search.rs`/`insertion.rs`/`type_filter.rs`/`state.rs`, a Stage-1 `Session`/`Intent`/`ViewRow`/`Host` API sketch, and verifiable Stage-1 exit gates. Companion to `CONTEXT.md`/`BEHAVIOR_MATRIX.md`/`TUI.md`; the eventual `WEBUI.md` documents the shared Web UI against this contract. (2026-06-17)
- Trimmed pure-duplicate prose from `CLAUDE.md` (492 → 464 lines): the `Insert` forming/clamp rules, the inline-vs-`$EDITOR` boundary setup, the array-element/multi-keyed paste forming, and the comments concept sentences now point to the `CONTEXT.md` *Insert / move legality* table and `BEHAVIOR_MATRIX.md` §6 instead of restating them. Operation-semantic mechanics (commit/rollback, paste state machine, kind-switch rules, inline-dotted machinery) are retained verbatim — a coverage-check subagent confirmed the target docs are glossary/matrix-oriented and never carried those. (2026-06-17)
- `CONTEXT.md`: reconciled an AoT-source discrepancy — the *Insert / move legality* table marked `[A/T]` sources ⏸ "not supported yet", but an AoT ***entry*** (`product[0]`) move/copy actually works (only a whole-***group*** Move is `Unsupported`). The ⏸ cells now read "group move `Unsupported`" and the note distinguishes group vs entry (entry splits into member fragments via `aot_entry_member_fragments`; a nested `[[…]]` sub-group move → `Unsupported`, copy → full capture). (2026-06-17)
- Doc restructure for the upcoming web-UI work: separated **model-layer** semantics (durable, UI-agnostic) from **TUI-layer** mechanics (ratatui-specific). `CONTEXT.md` gained `## Mutation mechanics` and `## Kind switch (K) rules` sections plus an inline-dotted-machinery paragraph; `CLAUDE.md` collapsed the Mutation/Kind-switch/Projection-dotted prose to `CONTEXT.md` pointers (464 → 365 lines). (2026-06-17)
- New `TUI.md` at repo root carries the eight TUI-specific sections (Rendering, Editing, Comments, Navigation, Filter, Type filter, Multi-select, Clipboard / paste) so a parallel `WEBUI.md` can land later against the same model contract; `CLAUDE.md` folded those sections to one-line `TUI.md` pointers (365 → 224 lines), leaving a skeleton of build commands, core model principles, module map, and cross-doc pointers. (2026-06-17)

## [v0.7.0] - 2026-06-17

### Fixed
- YAML: **cut/delete of a merged multi-line `#` comment block removed only its first line** — a 3-line block projects as ONE Comment node, but `delete_comment_token` spliced out just the single COMMENT token (+ its NEWLINE/INDENT), leaving the rest behind (and a cut-paste then re-inserted the whole block, duplicating lines). `delete` now removes the **whole** block via `delete_comment_block` (the same `comment_block_bounds` span the edit/remark path uses). (2026-06-16)
- YAML: **moving/pasting a node or comment past a top-level mapping's leading `#` comment landed it one slot too far**, and a cut comment block could mangle a neighbouring entry. A leading ROOT-level comment is projected as a root child but lives *outside* the top `MAPPING`/`SEQUENCE` (the edit container), so the projection index space — which `target.index` and the move `shift` use — ran one ahead of the container's own slot space per leading comment block. Insert/insert-comment now translate the projection index to container-local by subtracting `root_prefix_offset` (the count of leading ROOT-level comment blocks before the container; 0 for any nested container), so a move/add/paste lands at the projected slot. (Combined with routing a cut comment through the whole-block delete + `InsertComment`, this fixes the `placeholder:`-wrapping and value-loss seen when cut-pasting a comment block.) (2026-06-16)
- YAML: a standalone `#` comment line sitting **immediately after an entry** (no blank line before it) was **silently dropped from the projection** — invisible, unselectable, unmovable. A scalar/block entry swallows its line's terminating NEWLINE *inside* its `MAP_ENTRY`/`SEQ_ENTRY` node (via `bump_trailing`), so such a comment's previous sibling is the entry node, not a NEWLINE token — and `is_standalone_comment` treated a node sibling as "trailing" and skipped it (only a *blank* line before the comment, which leaves a NEWLINE sibling, saved it). It now looks *inside* the previous entry: standalone iff that entry's **last token** is a NEWLINE (scalar/block entries end on their line's NEWLINE; a flow `}`/`]` entry whose same-line trailing comment is genuinely trailing ends on the bracket). The comment was always preserved on save (it lives in the lossless CST) — this was a display/edit gap only. (2026-06-16)
- JSON/JSONC: a **merged multi-line `//` comment block** could not be external-edited (`e`/`E` → "path not found"), and **adding (`a`) below such a block** landed the new node too high by (comment lines − 1). Both stem from `collect_items` (the item list backing insert/move/edit-comment) counting each `//` line as a *separate* item, while the projection merges consecutive standalone `//` lines into **one** Comment node (one slot) — so item-space and the TUI's slot-space disagreed by the extra comment lines. `collect_items` now mirrors the projection: consecutive standalone `//` lines merge into one item (a blank line splits the block; a trailing same-line comment and `/* */` blocks stay their own item), so `EditComment`'s `\n`-joined block text matches its item and every insert/move index lands at the projected slot. (2026-06-16)
- TOML/JSON/YAML: **moving (cut) a node down past a trailing comment** landed it *after* the comment instead of at the requested slot, and **moving a comment node downward** overshot by one row. Both stem from `target.index` being a *pre-deletion* ordinal in the parent's full child sequence (comments occupy slots) that wasn't fully compensated once the source(s) were deleted before the re-insert. Fixed on three fronts: (1) TOML `move_nodes`' stable keyed-anchor search skipped comment slots, so it jumped past a trailing comment — it now subtracts the count of non-source comment slots between the target and the anchor (`gap`); (2) JSON/YAML `move_nodes` only decremented the insert index for positional (`Seg::Index`) sources, leaving a keyed node moved down past a comment unadjusted — they now compute a pre-deletion `shift` over *every* same-container source below the target (keyed and positional alike, via the projection so the index space matches the TUI); (3) the `do_paste` **comment phase** applied `InsertComment` at the raw, un-shifted `target.index`, so a cut comment whose source sat above the target overshot by one — it now drops the index by the count of already-deleted same-parent sources (node moves + the comment's own source) below the target (cut only; copy deletes nothing). (2026-06-16)
- TOML/JSON: external-editing (`e`/`E`) a node **reached through a standard-array index** (`arr[0].a`, `arr[0].a.b`) rewrote the **whole array** instead of just that node — only YAML was precise. `external_edit_path` truncated any path crossing an `Array` index back to the array, a leftover conservatism from before array elements / inline-table members were `Replace`-addressable. They are now (the inline splice rebuilds the enclosing `{ … }`/`[ … ]` element in place — verified for inline-table members, deep `a.b` nesting, multiline-string members, and array-of-arrays), so the truncation is dropped: the whole path is kept and the edit lands precisely, matching YAML. A standard-array *element* itself (`arr[0]`) is unchanged — still wrapped as the value-Replace form. (2026-06-16)
- TOML/JSON: a **plain-array element nested under a key** (`array_int[1].vals[0]` — an array that is a member of an inline-table element of a multiline array) opened `$EDITOR` instead of the **inline editor**. `edit_target_kind`'s array-element gate required `Key+ Index*` (no `Key` after the first `Index`); that restriction was over-conservative — `Replace` addresses such an element directly (`Target::ArrayElement`), verified across backends. The gate is now simply "immediate parent is a plain `Array`", so a single-line scalar element is inline-editable wherever the array sits (an AoT group is `ArrayOfTables`, not `Array`, so its entries still go `$EDITOR`; a multiline-string element still routes to `$EDITOR` by its Format). (2026-06-16)
- TOML/JSON: external-editing (`e`/`E`) a **standard-array element** rewrote the **whole array** instead of just that element — only YAML was precise. `edit_node` truncated an array-element path to the array because a bare element repr isn't `Replace`-addressable on its own in TOML/JSON. Factored the path/wrap decision into `App::external_edit_path`: an array element now keeps its own path and, on commit, its edited repr is wrapped as the backend's value-Replace form (`scalar_fragment(None, …)` → TOML `__elem__ = …`, JSON a bare value) so `Replace` splices only that element. YAML keeps its `- value` fragment with no wrap. AoT entries (parent is array-of-tables, not a standard array) and keys reached *through* an array index are unchanged. (2026-06-16)
- TOML/JSON: a **`[T/I]` inline table / inline object nested in an `[A/M]` multiline array** is now fully editable. A member (`arr[0].a`) edits **inline** with `e` instead of opening the whole array in `$EDITOR` (the projection already indexes the member, and the splice rebuilds the `{ … }` in place; `edit_target_kind` now treats a single-line member of an inline container as addressable even under an `Array` ancestor). The `[T/I]` element itself edits inline as its one-liner — and for JSON, an inline object (`NodeKind::Table` + `Format::Inline`) is now recognized as inline-editable. **Adding** a member into such an inline table no longer fails with "operation not supported": `inline_table_insert` accepts a `Target::ArrayElement` inline table (not only a keyed `Target::Entry`). (2026-06-16)
- TOML: editing a **`[T/I]` array element inline dropped its trailing comment** (`{ a = 1 },  # note` → comment lost). The projection attached an inline table's / array's EOL comment only when it sat inside the VALUE, but taplo attaches it to the **ENTRY** — so `split_value_comment` reported no comment and the editor staged a "comment cleared". `project_entry`/`project_entry_into` now pick up the entry-level trailing comment as a fallback, so it round-trips through an inline edit. (2026-06-16)
- TOML: adding (`a`) beside an **array element** seeded a `{ __elem__ = "" }` inline table instead of a bare keyless scalar, because `scalar_fragment(None, …)` returns TOML's value-Replace form (`__elem__ = value`). A new `ConfigDocument::array_element_fragment(value)` facet returns the **bare element** form per backend (TOML/JSON re-wrap a bare value and splice it keyless; YAML's `- value`), so all three formats seed array elements uniformly. (2026-06-16)
- YAML: inserting a member into a **`[T/F]` flow map nested in an `[A/F]` flow sequence** (`a: [{x: 1}]`) failed with "path not found" — `find_container`'s positional descent only handled block `SEQ_ENTRY`. It now descends an `Index` through a `FLOW_SEQ` to its i-th element node (`flow_seq_element_node`), so the inner `{…}` is addressable and the insert rebuilds it inline. (2026-06-16)
- YAML: converting an **`[A/F]` flow sequence whose elements are `[T/F]` flow maps** back to a block `[A/B]` failed with "operation not supported" — `block_members_from_flow` rejected nested `{`/`[` and split naively on commas. It now splits on **top-level** commas (depth/quote-aware) and keeps each nested flow element verbatim, so `a: [{x: 1}, {y: 2}]` expands to a block sequence of flow maps (the symmetric inverse of the already-working forward direction). (2026-06-16)
- `a` (add): pressing **Esc** in the inline editor opened by a fresh add now rolls the just-inserted seed back so the add leaves no trace (no undo/redo crumb), instead of leaving an empty placeholder node. An edit of an existing node is unaffected. (2026-06-16)
- YAML: deleting one element of an inline **`[A/F]` flow sequence** (`a: [1, 2, 3]` → delete `[1]`) wiped the whole sequence and reprojected the key as `[S:null]`. A flow-seq scalar element shared the *entire* `FLOW_SEQ` node as its resolver target (unlike a block `SEQ_ENTRY`), so the delete removed the sequence. `delete`/`replace` now detect a `FLOW_SEQ` target, take the element ordinal from the path's trailing index, and rebuild `[…]` without/with that element (`delete_flow_seq_element`/`replace_flow_seq_element`, mirroring the flow-map member path) — siblings and the key survive. (2026-06-16)
- TOML: adjacent standalone `#` comment lines **inside a multiline array** (`[A/M]`) projected as one node per line instead of merging into a single multi-line Comment node like comments at the table/document scope do. `project_array` now merges consecutive `#` lines (a blank line splits the group), and `comment_block_range` steps over the indent whitespace between an array comment's lines so edit/delete act on the whole block. (2026-06-16)
- `a` (add node) on a **collapsed branch** mis-placed the new node: it inserted a `new_field` scalar in the branch's *parent* scope, clamped to before any tables — so adding beside a collapsed array dropped a stray scalar two rows up. `a` now adds a **next sibling of the cursor's own kind** in the same scope (scalar beside a scalar, an empty `[]` array beside an array, a `[table]`/`{}` map beside a table, `[[aot]]` beside an array-of-tables, another comment beside a comment); the **root or an expanded branch** still appends an empty scalar as its last child. Container seeds go through the backend's `scalar_fragment` (no hard-coded notation), and an array's element seed is now keyless. (2026-06-16)

### Changed
- Internal: the TUI no longer **name-checks a backend** (`DocFormat == Toml/Yaml`) to decide editing behavior. Three new `ConfigDocument` facets carry the differences so a 4th format stays purely additive: `empty_container_fragment(kind, key)` (the `a`-add seed — TOML `[table]`/`[[aot]]`, JSON/YAML `{}`/`[]` — replacing the hard-coded notation in `app.rs`), `array_elements_addressable()` (YAML `true`: every seq element / scalar-under-a-seq is individually `Replace`-addressable — drives the inline-vs-`$EDITOR` routing and the external-edit element wrap), and `rename_can_change_type()` (TOML `true`: a dotted-key rename can turn a scalar into a `[T/D]` table). No user-visible behavior change. (2026-06-16)
- The TUI **KIND column** no longer prefixes the key-sign facet (`(B)/(Q)/(D)/(-)`); it shows only the 8-column type/notation slot (`[I:dec ]`, `[A/M]`, …). The key sign now reads as a word on a new **`Sign:`** line in the detail popup. The detail popup's **`Path:`** line now includes positional indices, so an array element reads `a.b[2].c` instead of the truncated `a.b.c`. (2026-06-16)

## [v0.6.0] - 2026-06-15

### Fixed
- JSON inline object mis-tagged `[T/S]` — a single-line JSON object projects as `Table` + `Format::Inline`, which fell through to TOML's scope-table default. `type_tag` and the type-filter's `classify` now carry a JSON arm: inline object → `[T/I]`, multiline → `[T/M]` (consistent with JSON's `kind_options`). (2026-06-15)
- YAML: editing a value with a trailing inline comment dropped the comment — YAML's `Replace` swaps the whole `key: value` entry, and the editor only re-asserted the comment when it had *changed*. New `ConfigDocument::replace_preserves_trailing_comment()` facet (default `true`; YAML `false`) makes the editor re-apply an existing comment after a value-only edit. The `←/→` value **nudge** does the same (it also goes through a value `Replace`), so a YAML int/float/bool toggle keeps its trailing comment. (2026-06-15)
- YAML: a single-line scalar inside a **block-sequence element** (`plugins[1].name`) opened `$EDITOR` instead of editing inline, and a block-map element captured the *whole* parent sequence as its `$EDITOR` fragment. The inline-vs-`$EDITOR` gate (`edit_target_kind`) and the fragment-capture truncation (`edit_node`) assumed array elements aren't `Replace`-addressable — true for TOML/JSON, false for YAML (the resolver descends `Index`→`Key`). Both are now backend-aware: YAML edits such scalars inline and captures just the element. `Replace` on a `Target::Element` now also accepts a `- `-prefixed whole-element fragment (reindent + byte-splice) so the element round-trips. YAML literal `|` / folded `>` block scalars now correctly route to `$EDITOR`. (2026-06-15)
- JSONC: a trailing inline comment could not be added to an **array element** — the comment was glued to the value and the type-check projection (`{"__k__": <val> // x}`) swallowed the closing brace (`expected R_BRACE`). Array elements now split value + comment like keyed scalars: a **multiline-array** element gains a real trailing comment (`1,  // x`, separator comma preserved across all backends), while an **inline** array / flow collection rejects it cleanly ("switch to multiline (K) first") leaving the document untouched. `set_trailing_comment` handles `Target::Element`/`ArrayElement` in all three backends; the editor seeds an element's existing trailing comment too. (2026-06-15)
- YAML: converting a **flow collection that is a sequence element** to block (`- {name: a, age: 5}` → block) emitted `-\n  name: a\n  age: 5` (an empty dash followed by an indented map, which reads as a stray blank line) instead of the canonical compact `- name: a\n  age: 5`. `convert_container`'s flow→block path now special-cases a map under a seq-element dash and writes the first member on the dash line. (2026-06-15)
- YAML (tracked-debt cleanup): keyed-vs-bare fragment detection (`item_key_name`, `parse_map_entry_fragment`, `adapt_fragment`) used a loose `contains(": ")`, so a quoted key holding `: ` (`"a: b": v`) keyed on the wrong span and a bare quoted scalar holding `: ` (`"a: b"`) was misread as a keyed entry. A new depth-aware `key_colon` helper finds the key/value colon outside quoted spans (honoring double-quote escapes and `''`), and all three sites route through it. (2026-06-15)
- YAML (tracked-debt cleanup): a **double-quoted key** with escape sequences (`"a\tb"`) projected with the literal backslash-`t` instead of the decoded character; the projection now decodes it via the shared `decode_double`. (2026-06-15)
- YAML: inserting a child **into a quoted-key mapping** (`"a b":` → add `y: 2`) failed with "path not found". The path segment carries the *decoded* key (`a b`), but `find_container`'s traversal matched it against the raw quoted token text (`"a b"`). It (and the `existing_map_keys` / rename sibling-collision checks) now compare against the decoded key via a new `entry_key_name` helper, so inserts/deletes/renames through and beside quoted keys resolve correctly. (Resolving a quoted key's own value already worked — projected paths and the resolver both use the decoded key; only these string-matching sites lagged.) (2026-06-15)

### Changed
- Internal (no behavior change): deduplicated three byte-identical `flush_comments!` macros in `yaml/project.rs` into one `flush_comment_block` helper; folded the `replace` "shouldn't happen" reparse fallback and the structurally-identical block-seq-element splice into a shared `splice_node_span`; extracted the duplicated PLAIN-token finder in `convert_int`/`convert_float` into `first_plain_token`. (2026-06-15)

### Added
- **Document-level cross-format conversion** (spec §Phase 4) — convert a loaded config to TOML, JSON/JSONC, or YAML through a common decoded intermediate (`src/model/value.rs`: `Value`/`Item`), emitted in the target's **default style**. Comments (standalone + trailing) carry across with the target's marker; notation/style differences (radix, string style, inline-vs-block, dotted keys, array-of-tables, exponent floats) are **normalized to default with an up-front lossy-warning list**; a TOML datetime becomes a quoted string into JSON/YAML (warned); and a conversion **aborts with no file written** when the source holds something the target can't represent — `null` → TOML (every null path listed) or a YAML opaque node (anchors/aliases/merge/tags) → any target. The rendered output is re-parsed by the target backend as a safety net before it is offered, and **the source document is never modified**. Two surfaces: a CLI command `confy convert <in> <out> [--from <fmt>] [--to <fmt>] [--yes]` (prints the warnings, asks y/n on a TTY or requires `--yes`, exits non-zero on abort) and a TUI action on the **Root node** (`C`): pick the target format, type the output path, then confirm past the warning list (the open document stays unchanged). The conversion engine lives in `src/model/convert.rs` (`tree_to_value` generic walk + per-format scalar decoders + the three default-style renderers); each backend implements `ConfigDocument::to_value`. (2026-06-15)
- **Trailing inline comments are now shown in-row and editable** (all backends). A trailing comment on a value (`host: x  # bind` / `"port": 8080  // http` / `key = "v" # note`) was captured in the model but only visible in the detail popup. It now renders **dimmed in the VALUE cell** after the value, and the **inline editor edits value + comment together**: the Value field is seeded as `value  # comment`, and on commit the buffer is split back (`ConfigDocument::split_value_comment`, which lexes via the backend so a `#`/`//` *inside a string* is not treated as a comment) into the value and the comment. A changed comment portion drives a new `Mutation::SetTrailingComment { path, comment: Option<String> }` (`Some` sets/changes, `None` clears) applied right after the value `Replace` as one undo step. The Normal-mode `←/→` value nudge (int/float increment, bool toggle) is unaffected — it still issues a plain value `Replace` that preserves the existing comment. A staged comment change is cleared if the edit is cancelled (`edit_cancel`), so it can never leak onto a later nudge/replace on another node. (2026-06-15)

### Fixed
- YAML: **nested flow collections** (`server: {host: a, inner: {x: 1, y: 2}}`) flattened — a `{…}`/`[…]` value inside a flow collection projected as `[S:null]` and its inner keys leaked out as siblings. The flow parser was a flat token bag (no per-member node); it now builds nested `FLOW_MAP`/`FLOW_SEQ` child nodes and a `FLOW_ENTRY` node per map member, so a nested flow value is a real, recursing child and each member is individually addressable. (2026-06-15)
- YAML: operations on a **flow-map member** (`ratio: {x: 1.5}` — editing/kind-switching/adding/deleting `x`) failed with "path not found" — every member resolved to the *whole* flow node, so `Replace`/`ConvertKind`/`Insert`/`Delete`/`Rename` had no sub-node to act on. Members now resolve to their own `FLOW_ENTRY`, and the edits rebuild the `{…}` inline (replace/insert/delete/rename keep the one line and the `, ` separators correct). Block-producing converts on an inline member (block expansion, literal/folded scalars) are rejected, and the `K` popup hides those options for in-flow members. (2026-06-15)
- YAML: editing or remarking a **leading/top-level comment** wiped the rest of the document. A leading comment parses as a direct child of the `ROOT` node, sitting beside the top `MAPPING`/`SEQUENCE`; `edit_comment`/`remark` rebuilt the container from its "slot items" only (which exclude that sibling node), so committing the edit overwrote `ROOT`'s whole text span and dropped the body. Both now replace just the comment block's exact byte span via a whole-document reparse (`splice_comment_block`), preserving every sibling. (2026-06-15)
- YAML: `e`/`$EDITOR` on a **block sequence or block mapping** failed to save with `unconsumed `  ` (INDENT) at top level`. `Replace` reparses the node's own serialized fragment (`flags:\n  - a\n  - b`); the keyed-fragment guard required a `: ` (colon-space) somewhere in the fragment, but a block-collection entry's first line is `flags:` (ends with `:`, value on the following lines), so the fragment was misrouted to the bare-value path and reparsed as malformed. The guard now inspects the first line and accepts a trailing `:`. (2026-06-15)
- Inline-edit/insert fragment-parse errors were always labelled `invalid TOML:` regardless of the loaded backend; the label is now format-aware (`invalid YAML:` / `invalid JSON:` / `invalid TOML:`) via a new `DocFormat::name()`. (2026-06-15)
- JSON inline editing rejected every commit with `invalid TOML: unexpected token` — the TUI hard-coded TOML `key = value` fragments (and taplo validation) for inline value edits, key renames, the `←/→` nudge, and the `a` seed, which the JSON backend's parser rejected. Fragment construction and the inline editor's type-change projection now go through two new format facets on `ConfigDocument` — `scalar_fragment(key, value)` (builds `key = value` / `"key": value` / a bare element) and `value_kind(value)` (projects the value in the backend's own syntax) — so the TUI never hard-codes a notation. The TOML-only dotted-key→table rename prompt is now gated to TOML. Editor validation errors read `invalid value:` (format-neutral) instead of `invalid TOML:`. (2026-06-13)

### Planned
- Multi-format backends — document-level conversion (Phase 4), XML out of scope. Spec: `docs/superpowers/specs/2026-06-12-multiformat-backends-design.md`. **Phases 1–3 (backend abstraction + JSON/JSONC + YAML subset) are now implemented** (below); Phase 4 (document-level conversion) remains planned. (2026-06-12)

### Added
- YAML subset backend (multi-format Phase 3) — a hand-rolled lossless lexer + recursive-descent parser onto `rowan` producing `YamlDocument` (`model/yaml/`): `load`/`serialize`/`apply` with atomic commit and a `validate_semantics` duplicate-key backstop, CST→NodeTree projection (golden tests), and one splice function per Mutation built on a **reindent engine** (YAML's analogue of JSON comma/brace normalization). **Subset:** a single document (optional leading `---`), block + single-line flow maps/sequences, 5 scalar styles (plain, single-quoted, double-quoted, literal `|`, folded `>` with chomping), `#` comments, and YAML 1.2 **core-schema typing** with **no datetime** (date-looking scalars are strings). **Out-of-subset constructs** (`&anchor`, `*alias`, `<<:` merge, `!tag`, multi-line flow) project as **read-only opaque nodes** (new `Node.read_only` flag, KIND tag `[opaq ]`): displayed and copyable, but every mutation on or into them returns `Unsupported` (document untouched). **Multi-document** files are rejected at load. New `Format` variants (`Block`, `SingleQuoted`, `DoubleQuoted`, `LiteralBlock`, `Folded`) and `KindTarget` variants (`Flow`/`Block`, `String{Plain,Single,Double,LiteralBlock,Folded}`) drive the KIND tags `[A/B]`/`[A/F]`/`[T/B]`/`[T/F]`/`[S:sq  ]`/`[S:dq  ]`/`[S:lit ]`/`[S:fold]`. `K` kind-switch covers map/seq block↔flow, the 5 string styles, integer radix (dec/hex/oct), and float plain↔exponent; the `f` type-filter shows only YAML-reachable facets (`(B)`/`(Q)`/`(-)` signs, block/flow containers, no dotted/AoT/datetime/binary). `type_tag` and the type-filter's `classify` now take `DocFormat` (and `read_only`) so the KIND column and the filter popup stay in lockstep across backends. `AnyDocument` gains a `Yaml(YamlDocument)` variant; `load_as` dispatches `.yaml`/`.yml` to it (the earlier "support is coming" bail is gone). (2026-06-13)

### Added
- JSON/JSONC backend (multi-format Phase 2) — a hand-rolled lossless lexer + recursive-descent parser onto `rowan` (same version taplo uses, added as a direct dep) producing `JsonDocument` (`model/json/`): `load`/`serialize`/`apply` with atomic commit and a `validate_semantics` duplicate-key backstop (DOM re-parse), plus CST→NodeTree projection (golden tests) and one splice function per Mutation. **JSONC** (`//` line comments + `/* */` block comments, trailing commas accepted on parse): `//` lines project as first-class Comment nodes (consecutive lines merge; a blank line splits them) or `trailing_comment`; `/* */` block comments project as **read-only** Comment nodes (the new `Node.read_only` flag — displayed and copyable, but edit/delete/cut/remark all reject them). A pure `.json` file that receives its first remark (`r`) prompts `Mode::Prompt(JsoncUpgrade)`; confirming (`y`) flips `supports_comments()` true and `//` is used thereafter (extension stays `.json`). New model atoms: `ScalarType::Null` (`[S:null]`), `Format::Exponent` (`[F:exp ]`), `KindTarget::TableMultiline` (`[T/M]`). `K` kind-switch for JSON: object/array Inline↔Multiline (`[T/M]`), float Plain↔Exponent. `f` type-filter shows only JSON-reachable facets (`(Q)`/`(-)` signs; no `[A/T]`/`[T/D]`/`[T/S]`/radix/string-style/datetime). JSON omits TOML-only features: no dotted keys, array-of-tables, datetimes, integer radixes, multiline strings, or string-notation switching; newlines are encoded as `\n`. `AnyDocument` gains a `Json(JsonDocument)` variant; `load_as` dispatches to it for `.json`/`.jsonc`. (2026-06-13)

### Added
- Backend abstraction (multi-format Phase 1) — a pure refactor that removes the TOML leaks between the TUI and the document layer so JSON/YAML can plug in later; **TOML behavior is byte-for-byte unchanged**. **`AnyDocument`** (`model/any_doc.rs`) is a one-enum dispatcher wrapping every backend (one `Toml(CstDocument)` variant today) and implementing `ConfigDocument` by match-delegation, so the TUI holds a single concrete type and a new format is one more variant. The trait gains **format facets** (`format() -> DocFormat`, `comment_prefix()`, `supports_comments()`) and **`kind_options(path)`**, which moves the `K` popup's per-node convertible-kind list into the backend (the TUI no longer hard-codes a format's notations). `Mutation::Insert`/`Replace` rename their `toml:` field to format-neutral `fragment:`; path→node lookup moves onto `NodeTree::node_at`; help text is keyed by `DocFormat`. The CLI now recognizes `.json`/`.jsonc`/`.yaml`/`.yml` (and `--format json|yaml`) but `load_as` **bails politely** ("… support is coming in a later release") until those phases ship; unrecognized extensions report clearly. (2026-06-13)

### Changed
- `[A/T]` ≡ array of inline tables — the two container kinds now behave as one. **Moving/copying a `[[entry]]` out of its group splits it into member nodes** (the old `[scope]`-section conversion is gone): body entries land one node each, **sub-sections flatten to dotted entries** (`[fruit.physical]` `color` → `physical.color`), into another group / an array the members pack into ONE `[[entry]]` / `{ … }` element; deleting an entry now removes its sub-sections with it. **Copying a `{ … }` array element into a table/root/`[A/T]` unpacks it into member entries** (matching the existing cut path; bare scalars keep the `placeholder` key). **`K` converts `[A/T]` ↔ `[A/I]`/`[A/M]`**: a contiguous comment-free group of plain single-line entries becomes `key = [{ … }, …]` (position-checked against the capture rule), and a keyed flat-ROOT array of inline tables becomes an `[[…]]` group (rejected when the sections would capture a following entry). (2026-06-12)

### Added
- `K` kind switch — a single-select popup converting the cursor node's notation in place (`Mutation::ConvertKind`; `k` stays vim cursor-up). **Scalars switch between notations of their own type**, never across types: strings between basic/literal/multiline/multiline-literal forms (content decoded then re-encoded; a `'` in a literal form, `'''` in a multiline literal, or a real newline in a single-line literal rejects as `Illegal` — single-line basic escapes newlines as `\n`, so mstr→str is lossless), integers between decimal/hex/octal/binary radices (negatives have no prefixed form), floats between plain ↔ exponent; bools, datetimes and `inf`/`nan` don't convert. **Arrays** toggle inline ↔ multiline (the collapse rejects interior comments and multi-line elements). **Tables** convert between `[T/I]`, `[T/D]` and `[T/S]` writing styles: `[T/S]` targets are checked against the table-capture rule (a `[t]` mid-entries, or a section preceded by a foreign header, rejects as `Illegal`), inline targets reject held comments, and a nested `[s.t]` converts relative to its parent's capture (`[s.t]` → dotted `t.a = …` under `[s]`). AoT entries, Root and comments are not convertible. (2026-06-12)

### Changed
- `[T/D]` comment binding reverted — **comments are never inside a `[T/D]` table again** (aligning with `[T/I]`): a comment directly above a dotted member is an independent scope-level node that stays put on table move/copy/delete and the `e` consolidation. A comment pasted "into" a `[T/D]` now lands at the scope level **directly above the table's first member** as an independent node (never rejected). (2026-06-12)

### Added
- Array paste alignment — **plain arrays now mirror the `[A/T]` interactions**: multiple copied/cut keyed nodes (or a `[T/D]` table's members) pack into **one** `{ a = 1, b = 2 }` inline-table element instead of one element each; **moving an inline-table element out of an array unpacks it into keyed entries** (`{ a = 1, b = 2 }` into a table → `a = 1` / `b = 2`, each per-leaf collision-checked; previously only a single-key `{ k = v }` unwrapped and a multi-key one got a `placeholder` key). Bare values keep their element form / `placeholder` key; `[T/S]`/`[A/T]` sections into an array stay `Illegal`. (2026-06-12)

### Added
- `[A/T]` interactions — **pasting keyed nodes into an array-of-tables group now synthesizes a new `[[…]]` entry at the target slot**: a keyed node, inline table or `[T/D]` table (its members fan out) lands inside the new entry; multiple copied/cut nodes are joined and **pack into one entry**. Keys never collide with sibling entries (each `[[…]]` opens a fresh namespace); duplicates *within* the pasted set follow o/r/c. A `[table]`/`[[aot]]` section into a group stays `Illegal`. **Moving a `[[…]]` entry out of its array is now supported**: the entry converts to a `[scope]` table — captured scope-relative (`[[a.b]]` → `[b]`), re-prefixed for the destination (`[s]` → `[s.b]`), partition- and collision-checked (landing it beside its own group is a `Collision`; rename yields `[p_2]`). Copy (`c`) of an entry captures the same `[k]` scope form; the `$EDITOR` block edit keeps the verbatim `[[…]]` header. (2026-06-12)

### Fixed
- Duplicate-key safety — **every mutation now runs a semantic backstop (taplo DOM validation) before commit**: taplo's parser is syntax-only, so a whole-document `E` rewrite or a block `e` edit could introduce a duplicate `[a]` section or re-defined key and be accepted. The result tree is now DOM-validated (conflicting keys → `Collision`, other semantic errors → `Illegal`), with the document left untouched on rejection; all legal layouts (scattered `[a] … [a.sub]`, dotted siblings, AoT re-openings, mixed `fruit.apple`) still pass. Also fixed the targeted pre-check for **section inserts into a sub-scope**: the header is re-prefixed to `[b.a]` before the collision check, but the check prepended `target.parent` again and looked up a phantom `b.b.a` — pasting `[a]` into `[b]` when `[b.a]` existed silently produced a duplicate section. (2026-06-12)

### Added
- Projection — **dotted keys inside inline tables now decompose into `[T/D]` chains**. `t = { x.y = 1, x.z = 2, w = 3 }` projects a synthetic `[T/D]` table `x` nesting `y`/`z` (members sharing a prefix merge), instead of flat `x.y`/`x.z` leaves. Operations on the synthetic node route through the inline-table machinery, never the flat-ROOT splices (which previously panicked on such paths): insert/add re-prefixes the key scope-relative (`q = 9` into `t.x` → member `x.q = 9`) with exact-full-path collision (a shared prefix merges); delete and move/copy fan out over the `{ … }` member entries (capture keeps the node's own key: cutting `t.x` to root yields `x.y = 1` / `x.z = 2`); the `e` block edit consolidates the members at the first one (single-line entries only); comments into it are rejected. (2026-06-11)

### Fixed
- Insert/Move — **an entry targeted into a `[T/S]` whose children are sub-sections no longer fails**. The paste "Into" slot appends at `children.len()`, which for a table like `[pt]` + `[pt.a]`/`[pt.b]` pointed past the section run and reported `Illegal("a key here would be captured by the table above it")` — e.g. moving a global `[T/D]` into `product_table`. The index is now clamped to the nearest legal slot for *table* destinations (an entry lands at the end of the table's own entry run, before its first sub-section; dually, a section targeted before the entries lands at the section run). Root-level inserts keep their strict position semantics. (2026-06-11)
- Insert — **copying a `[T/D]` table into an inline table no longer drops members**. The multi-entry fragment split ran *after* the inline-table branch, so only the first member was spliced into the `{ … }` (move was unaffected — it fans out per member before inserting). The split now runs first, and the per-entry landing slot is held by a stable anchor path, fixing a second drift where copied members landed non-contiguously in a scope destination (inserted dotted entries merge into one projected child, so `index + k` overshot later siblings). (2026-06-11)

### Added
- `[T/S]` discretization — **a table's definition is now treated as an open set of "member spans"** (its own `[a]` section, every scattered descendant `[a.sub]`/`[[a.list]]` section, and any flat dotted member lines), unifying `[T/D]`, `[T/S]`, implicit and mixed tables under one mechanism (`table_member_spans`). Serialize/`e`-edit, delete, and move/copy all fan out over the full span set: deleting or cutting a scattered `[a] … [b] … [a.sub]` no longer leaves an orphan `[a.sub]` behind (which silently re-created `a`); the `$EDITOR` block edit on a scattered `[T/S]` captures **all** of its sections and consolidates them at the table's first definition; scattered `[[a.list]]` AoT sub-groups travel with the table in entry order. A consolidating block edit is validated: every header must stay inside the table's subtree and the block must be header-led, else `Illegal` (the document is untouched). (2026-06-11)
- Implicit scope table — **an entry can now be inserted into a header-less table** (only `[a.sub]` was ever written): its own `[a]` section is synthesized at the table's first definition to hold the entry (`[a]` + `x = 1` before `[a.sub]`). `e`/copy on an implicit table now captures its sections instead of returning an empty fragment. (2026-06-11)
- Mixed table (dotted members + header sub-sections, the TOML-spec `fruit.apple` pattern) — first-class support: **`e` consolidates it to scope form** (`[fruit.apple]` with the dotted members folded under it, then the member sections — the only header form that leaves no dotted definitions behind, which the spec forbids alongside a header); **inserting an entry writes a dotted member** next to its siblings (never a header while dotted definitions remain); **inserting a sub-table is now allowed** (previously rejected whenever the parent projected as `[T/D]`); delete/move fan out over members and sections. Also fixes a corruption: a descendant section's entries were mis-counted as dotted members, so `e`/delete on the table ripped `smooth = true` out of its `[fruit.apple.texture]` context. (2026-06-11)

### Changed
- Consolidation anchor — **`[T/D]` tables now project and consolidate at their *first* definition** (previously last): the block edit on `a.b = 1 / x = 0 / a.c = 2` lands the rewritten block where `a.b` was, and the projected tree shows `a` before `x`. `[T/S]`/mixed consolidation anchors at the first member *section*. Projection slot and rewrite landing point stay paired, as before. (2026-06-11)
- Clipboard/move capture — **a nested `[T/S]` table is now captured scope-relative**: cutting `[a.sub]` into `[b]` yields `[b.sub]` (headers drop the source ancestor path, mirroring the `[T/D]` prefix strip) instead of stacking the old path. Cutting a child out of a *scope-first* mixed table now also drops the table's key prefix (the headerless-ancestor rule replaces the `Format::Dotted` check). (2026-06-11)

### Changed
- TUI — **`[T/D]` dotted tables now start collapsed** like every other branch, instead of being seeded open at load. Only the root file node starts expanded; a dotted key shows just its top segment until expanded (`1`/`9`). (Removed `seed_dotted_expanded`.) (2026-06-11)

### Added
- Cross-type table moves — **moving a `[T/S]` scope table into another scope now nests it**. `[a]` (and any nested `[a.sub]`) moved into `[b]` becomes `[b.a]` (`[b.a.sub]`): every header in the moved section is re-prefixed with the destination path (`prefix_section_headers`). Moving a `[T/D]` dotted table into an inline table already flattened its members into inline dotted keys (`t = { …, a.x = 1, a.y = 2 }`); this stays. (2026-06-11)
- Move/copy a whole `[T/D]` dotted table — **a synthetic dotted table can now be cut/copied as a unit**. Moving one used to fail (`NotFound`); it now fans out to its member entries, each captured scope-relative and re-prefixed for the destination, so moving `[T/D]` `a` into a scope drops the prefix, into another `[T/D]` `b` adds `b.`, and out to root strips it. Copy works too: a header-less multi-entry fragment is inserted one entry at a time (a single splice previously dropped all but the first member). (2026-06-11)
- Move out of an array — **array elements can now be moved/cut**. Cutting an array element and pasting it elsewhere used to be `Unsupported`. Into another array it stays a bare element; into a table/root a **single-key inline table** (`{ k = v }`) unwraps back to a keyed entry `k = v` (the inverse of the insert-into-array wrapping), while a multi-key inline table or a bare value gets a synthesized `placeholder` key. The destination format is then applied normally (e.g. a `[T/D]` dotted-table destination re-prefixes the key). (`unwrap_single_key_inline`.) (2026-06-11)

### Changed
- Insert into array — **a keyed node now keeps its key**. Pasting/moving a keyed entry into an array used to drop the key (keeping only the value); it is now wrapped as a `{ key = value }` inline-table element (a keyed inline-table value becomes a nested inline table). A *keyless* bare value (scalar, inline table, or array) still becomes the element as-is; a `[table]`/`[[aot]]` header is still rejected. (`wrap_keyed_as_inline_element`.) (2026-06-11)

### Fixed
- Insert — **illegal table-into-container moves now report a clear message** instead of a generic `NotFound`/silent no-op. A `[table]`/`[[aot]]` section inserted into an inline table errors "a table cannot be inserted into an inline table"; nested under a `[T/D]` dotted table it errors "a scope table cannot be nested under a dotted table". (2026-06-11)
- Insert — **a dotted entry sharing a prefix no longer false-collides**. Collision is now decided on the inserted leaf's **exact full path** (`target.parent ++ key segments`) instead of just the first key segment, so `a.y` inserted next to an existing `a.x` merges into the same `[T/D]` table rather than reporting a collision on `a`; an identical full key (`a.x` over `a.x`) still collides. (2026-06-11)
- Insert/Move — **pasting or moving a node into the slot *before* a `[T/D]` dotted table no longer fails silently**. A synthetic `[T/D]` table has no backing CST element, so `resolve_insert_at` returned `Unsupported` when asked to anchor an insert before it — manifesting as a stuck paste (`v` did nothing) when the destination line sat between a node and a following `[T/D]` table (e.g. cut a scalar, paste after a multiline array immediately followed by a dotted table). The anchor now descends to the table's first member line (`node_start_root_index`). (2026-06-11)
- `[T/D]` editing — **an inline-table value member no longer leaks its contents on block edit**. A `[T/D]` table member whose value is an inline table (`dotted.t = {x=1}`) had its interior `x=1` mis-counted as a flat dotted member by `dotted_member_entries`, so the `$EDITOR` block edit (and the table fragment) pulled `x=1` out as a stray top-level line. The member scan now skips any entry nested inside an `INLINE_TABLE`/`ARRAY` value, restoring the documented "flat-ROOT" rule. (2026-06-11)
- Clipboard — **copying/cutting a node out of a `[T/D]` table now drops the dotted-ancestor prefix**. A leaf like `dotted.test.bool_true` is captured as `bool_true` (the `test` subtable as `test.bool_true`), so pasting it into a normal table yields `bool_true = true` instead of `dotted.test.bool_true = true`; pasting back into a dotted table still re-prefixes for that destination. Copy uses a new scope-relative `serialize_fragment_relative`; cut (`Mutation::Move`) strips at capture. The `$EDITOR` block edit keeps full keys (unchanged). (2026-06-11)
- Insert — **pasting/adding a keyed node into an inline table now works** instead of silently doing nothing. A new `inline_table_insert` rebuilds the `{ … }` from its members' verbatim source with normalized `, ` separators, splicing the new entry at the target position (front/middle/append) and rejecting a duplicate key; an empty `{}` becomes `{ k = v }`. (2026-06-11)

### Added
- Projection — **dotted-key tables now nest as `[T/D]` containers**. A multi-segment dotted key (`a.b.c = 1`) projects as a chain of synthetic `Table` nodes (`a → b → c`) instead of one flat `a.b.c` leaf; scattered dotted entries sharing a prefix merge under one table **per scope**, positioned at the table's **last** definition in that scope (where a block-rewrite places it). They navigate, filter and expand like real tables, carry a new `Format::Dotted` facet (`[T/D]` in the KIND column, its own checkbox under Tables in the `f` filter), and start collapsed like any branch. The whole decomposed chain (synthetic tables **and** leaf) reads the dotted key sign `(D)`, so the `f` filter's `(D)` checkbox matches decomposed dotted entries; a dotted key **inside an inline table** stays one `(D)` leaf (not decomposed). **An untouched file still round-trips byte-identically** — each leaf maps to its original source entry; only the operations below rewrite anything. (2026-06-10)
- Editing `[T/D]` tables (round 2):
  - **Add** (`a`) on a `[T/D]` table seeds a scalar (`new_field = ""`), not a `[placeholder]` table — a dotted table opens no scope, so a following scalar is legal. Child inserts/paste write a scope-relative dotted entry (`x = v` → `a.b.x = v`) next to existing siblings. (2026-06-10)
  - **Rename** a plain key to a dotted one (`foo` → `foo.x`) converts the scalar into a `[T/D]` table in place; the inline editor confirms the `integer → table` change first and `n` leaves the document untouched (`Mutation::Rename` now rewrites the whole key, introducing dots). (2026-06-10)
  - **`e` (block edit)** on a `[T/D]` table opens `$EDITOR` with all of its dotted member lines and, on save, **consolidates** them — removing the scattered entries and writing the edited block at the table's last position. (Standalone comments between members stay where they are.) (2026-06-10)
  - **Delete** (`d`) on a `[T/D]` table removes all of its member entries (plain cascade); deleting the last member drops the now-empty table too. (2026-06-10)
- TUI — **`f` type-filter checkbox popup**. `f` opens a modal menu for filtering the tree by a node's type facets — the same facets the KIND column shows. Two halves: **key sign** (`(B)/(Q)/(D)/(-)`) and **type** (`[G]` root, `[C]` comment, arrays, tables incl. `[A/T]`, strings, integers, floats, bool, dates), each multi-format group with an **`all`** quick-toggle row. Selections within a half **union**; the two halves **intersect** (AND); an empty half is no constraint. The `all` rows are tristate (`[x]`/`[~]`/`[ ]`). Arrows move the cursor (header rows skipped), Space toggles, the tree filters **live** in the background, Enter locks the result into the existing `FilterResults` browse mode, Esc peels the type filter off. Composes with the `/` text filter via AND intersection; when both are active Esc peels **one layer at a time** (most-recently-applied first) and the status bar shows both `[filter: …]` and `[type: N]`. New `tui/type_filter.rs` module (`TypeToken`/`classify` mirror `type_tag` so the popup and KIND column can't drift); new `Mode::TypeFilter`. (2026-06-10)

## [v0.5.0] - 2026-06-10

### Added
- TUI — **`1`/`2` level-by-level expand/collapse**. `1` reveals one more depth level of the branch under the cursor (subtree-scoped, shallowest unexpanded level per press, until full) — distinct from `9` which expands the whole tree at once. `2` collapses one level and climbs: an open branch under the cursor collapses in place (cursor stays); otherwise the cursor moves up to its parent branch and collapses that, so repeated presses ascend the tree. Both are pure view-state (no document mutation), mirroring `9`/`0`. (2026-06-10)
- TUI — **KIND column header, 40% NAME column, and scrollable help overlay with KIND legend**. The `TYPE/FORMAT` column header is renamed to `KIND`. The NAME column now takes a fixed 40% of the terminal width (`name_col_width = total * 2/5`, floor 10) instead of an equal split with VALUE — KIND starts at the 2/5 mark and VALUE gets the wider remainder. The `?` help overlay is now scrollable (`↑/↓/PgUp/PgDn/Home/End`; `help_scroll` in `App`, reset on open) and its title advertises scrolling; the popup is widened to 65% to accommodate the new KIND legend section (key signs `(B)/(Q)/(D)/(-)`, container slots, and all scalar slots with one-line meanings). Name-field inline edit width is now driven by `name_col_width` instead of `value_col_width`; the per-frame scroll clamp in `mod.rs` picks `name_col_width` when editing the Name field. (2026-06-10)

- Comments — **inserting a comment into a single-line array now upgrades the array to multiline** instead of rejecting (reconstruct increment 3). `Mutation::InsertComment` reformats the array one element per line (elements keep their exact source repr; an end-of-line comment after the `]` stays put) and then splices the comment in at the requested slot — still atomic, valid TOML 1.0. In the TUI, pasting/moving a comment onto a single-line array asks first (`Reformat array to multiline and insert? y/n`): `y` re-issues the paste with the upgrade allowed, `n` cancels keeping the clipboard, so a cut is never destructive. The inverse (collapse back to inline when the last comment is removed) is intentionally not built. (2026-06-10)
- TUI — **the TYPE/FORMAT column now renders fixed-pitch tags** (reconstruct increment 2): a 3-char key sign `(B)`/`(Q)`/`(D)`/`(-)` plus an 8-char type slot — containers `[G]` root, `[C]` comment, `[A/I]`/`[A/M]` inline/multiline array, `[A/T]` AoT, `[T/I]` inline table, `[T/S]` table scope; scalars `[S:str|mstr|lit|mlit]`, `[I:dec|hex|oct|bin]`, `[F:flt|inf|nan]`, `[B:bool]`, `[D:odt|ldt|ldat|ltim]`. Always exactly 12 columns, so the column never shifts (`type_tag` in `app.rs`, fed from `NodeKind` + `Format` + `KeySign`). The detail popup keeps the human-readable word labels, and the inline editor's type-change detection still compares word labels. (2026-06-10)
- Model — **`KeySign` facet and container `Format`s** (reconstruct increment 1). Every projected Node now carries `key_sign: Bare | Quoted | Dotted | None` (how its own key is written; `None` for keyless nodes — array elements, comments, AoT entries, Root), derived read-only from the key tokens during projection. `Format` extends beyond scalars: arrays project `Inline` vs `Multiline` (explicitly, instead of inferring from `value.is_none()`), inline tables `Inline`, `[table]` scopes `Scope`, and `inf`/`nan` floats get their own `Inf`/`Nan` formats. Golden tests in `cst_project.rs` regenerated (they now print `sign=`) plus a new golden freezing the new facets. (2026-06-10)

### Fixed
- TUI — **the `?` help overlay could not be scrolled to the bottom**; the lower KIND-legend lines (scalar tags) were unreachable. The shared `centered_rect` helper placed the popup at the vertical middle (`y = height/2`) and capped its height to the remaining ~half-screen, while the scroll clamp assumed a full-height box — so `max_scroll` was far too small. `centered_rect` now centers vertically and uses the full requested height (mirroring `detail_popup_rect`), making the scroll math correct and the whole legend reachable. (2026-06-10)

### Removed
- **The legacy `toml_edit` backend is fully retired** (CST migration Phase 5/6 complete). `toml_doc.rs`, `project.rs`, `fragment.rs` and the `toml_edit` dependency are deleted; `CstDocument` (taplo/rowan) — live since v0.4.0 — is the only backend. The migration-era projection-parity tests are frozen as golden tests in `cst_project.rs`; `tests/roundtrip.rs` now runs against the CST backend. (2026-06-10)

### Changed
- Internal cleanup of vestigial toml_edit-era machinery: the `sync_decor` flag on `Mutation::Replace` and the `carry_comment` flag on `ConfigDocument::serialize_fragment` are removed (comments are independent CST nodes, so a value replace can never disturb one and a fragment never carries one); the `clipboard_fragment`/`strip_leading_comment_block` helpers and the last `#comment:N` path-sniffing in the filter are gone. The inline editor's type-change detection now parses the fragment with taplo and reuses the projection's type labels (`node_type_label`). No user-visible behaviour change. (2026-06-10)

### Docs
- `CLAUDE.md` architecture section rewritten for the CST backend (comments as first-class nodes, `Seg::Index` addressing, atomic apply); the obsolete decor-machinery paragraphs are gone. (2026-06-10)

## [v0.4.0] - 2026-06-09

### Added
- Multiline arrays — **interior comments are now first-class nodes.** A standalone `# …` line inside a multiline array projects as a Comment node (sharing the element index slots), so it is visible, editable (`e`/`E` → `EditComment`), deletable (`d`), and can be **pasted/moved into** a multiline array (`v`) — landing on its own indented line; a comment on the *same line* as an element becomes that element's trailing comment. Single-line arrays still reject comments (a `#` would comment out the `]`), and a cut into an illegal target aborts non-destructively. (`project_array`/`array_insert_comment` in `cst_edit.rs`.) (2026-06-09)
- Editing — **single-line arrays and inline tables now show their value in the VALUE column and edit inline.** A one-line `[1, 2, 3]` / `{ x = 1 }` is projected with its repr as `value` (multiline arrays keep `None`), so `e` edits it in place as a one-line field and commits a structured `Replace`; `Tab` still renames the key. Multiline arrays and nested structured array-elements stay in `$EDITOR`. (`project_array`/`project_inline` in `cst_project.rs`.) (2026-06-09)
- Clipboard — **cross-layer paste now adapts the node to the destination container** (simple cases). Pasting a keyed scalar/array/inline-table **into an array** drops its key and inserts the value as a bare element (`key↓`); pasting a **bare array element into a table/root** synthesizes a `placeholder` key (`key+`), auto-suffixed (`placeholder_2`, …) on collision without a prompt. Hard coercions stay rejected: a `[table]`/`[[array]]` cannot become an array element. (Phase C of cross-layer ops; `parse_fragment_adapted` in `cst_edit.rs`.) (2026-06-09)
- Clipboard — paste mode now targets a **precise insertion slot** instead of a whole row. `↑/↓` step through a merged sequence of slots: a **standalone green line between two nodes** = insert as a sibling *after* the row above it (the line is indented to the depth it will land at, and the nodes' own text is never restyled), and a **whole branch row turning green** = append as that branch's **last** child (open or collapsed). The green-line state cannot toggle the branch above it (Enter/Space is a no-op there); only the green-branch (`Into`) state toggles. The slot defaults to the old cursor-relative position right after copy/cut, so existing paste behaviour is unchanged until you move it. (Phase A of cross-layer ops; `PasteSlot` in `state.rs`, slot resolution in `app.rs`, render in `ui.rs`.) (2026-06-09)
- Filter — `/` is now a three-state flow. Typing in the input filters live; **Enter** locks in the filtered set and enters a filtered-result selection mode (navigate/select/edit on the filtered nodes while the status bar shows `[filter: …]`); **Esc** clears the filter back to the full list; **`/`** reopens the input (prefilled) to refine. The last committed query is remembered, so `/` restores the previous search and its live results. (2026-06-07)
- TUI — the root/file node (`▾ test.toml`) is now collapsible like any branch (Enter/Space toggles `▾`/`▸`); it starts expanded. `0` (collapse all) keeps the file node open; an explicit toggle on its row hides the whole document. (2026-06-07)
- Filter — while a filter is active, the fuzzy-matched characters are highlighted (bold/underlined) in the NAME cell (`search::fuzzy_indices` + `ui::highlight_spans`), so it's clear why each row matched. The highlight persists through an inline edit or detail popup opened from the filtered list (gated on the active query, not the mode), and closing the editor/popup returns to the filtered-result selection (`App::resting_mode`) instead of dropping to plain Normal. (2026-06-07)
- Clipboard — pressing `c`/`x` while a clipboard is already loaded now **toggles** its mode (copy ↔ cut) instead of re-capturing the selection, so a mis-pressed `x` can be corrected to `c` without re-selecting. The status bar reflects the change. (2026-06-08)
- Clipboard — **comment nodes can now be copied/cut/pasted** like any other node. A comment serializes to its raw `# …` text and pastes via a new decor-aware `Mutation::InsertComment` (it lands by the same rule as other nodes and never collides, since comments have no key). For a cut, the source comment is deleted before re-insert so an identical comment elsewhere isn't disturbed. (2026-06-08)
- Clipboard — a cut node pasted into a table now lands at the **cursor position** (exact-position reorder), not appended at the end. Table positioning uses the order-preserving rebuild technique, with the insertion point resolved against the pre-move tree so a same-table reorder isn't thrown off by the source's own removal. (2026-06-08)

### Changed
- TYPE column — an inline table now reads as **`inline-table`** (was `table/inline`), parallel to `array-of-tables`. Display-only; the internal type label is unchanged. (2026-06-09)
- Add (`a`) — now routes by the cursor's state instead of always seeding a first child. An **expanded** branch (or the root) appends the new node as its **last** child; a **collapsed** branch or a leaf inserts it as the **next sibling**. The seed is still an empty-string scalar opened in the inline editor, except where a scalar would break TOML's table-capture rule at that slot — `a` on a **collapsed `[table]`/`[[array]]`** now adds a **same-kind structured sibling** (`[placeholder]` / `[[placeholder]]`) instead of an illegal scalar. Appending a scalar into a branch clamps it to the leading region, so **adding a root-level scalar now lands before the first table** (previously it could be silently captured). (Phase D of cross-layer ops.) (2026-06-09)
- Clipboard — moving (cut+paste) or copying a node no longer drags the **comment node(s) above it** along. Standalone comments live inside the moved node's leading decor (a leaf's `leaf_decor`, a `[table]`'s header decor), so previously they travelled to the destination and were erased from the source. A move now leaves the node's **entire leading prefix** behind — including several stacked comment blocks (e.g. a top-of-file banner) and duplicate comment texts — re-homed onto the source's next sibling, or onto the document trailing when the source was the last top-level key (`detach_leading_comments` in `toml_doc.rs`, inside the atomic `Mutation::Move`). A copy drops the leading comment block from the fragment (`clipboard_fragment` in `app.rs`). Copying a comment node itself is unaffected. Known edge: moving the **last** key out of a *nested* `[table]` keeps the old carry-along behaviour. (2026-06-08)
- Editing — opening a **scalar** in `$EDITOR` (`E`, or a multiline string) now carries its adjacent leading comment into the editor and writes edits/deletes to that comment back to the file, matching the existing behaviour for tables/arrays. Inline edits (commit, `←→` nudge, type-change confirm) are unaffected and never disturb the comment — `Mutation::Replace` gained a `sync_decor` flag that the `$EDITOR` path sets and the inline path clears. (2026-06-08)
- Filter — the fuzzy query now matches a node's **key/path** (plus a **Comment node's own text**, so comments stay searchable as standalone nodes), but **no longer a scalar's value** (and the synthetic `#comment:N` key is excluded). A loose query like `array` previously fuzzy-matched unrelated values (e.g. `…color = "gray"`) and the value+comment duplicate in the haystack dragged in unrelated section comments, which also made them look "scattered"; now `array` surfaces the `array_*` keys and the array-related comment only. (2026-06-07)
- Multi-select — each Shift+Arrow run now starts a fresh range anchored at the cursor and **unions** onto previous selections (separate runs stay separate, overlapping runs merge), instead of every new run extending from the first run's anchor. `Esc` in normal mode now clears the active selection. (2026-06-07)
- Clipboard — selection mode and clipboard mode are now cleanly separated. While a clipboard is active, `s` and Shift+Arrow are locked (no selection changes); the cursor row is shown green (paste-ready) and the copy/cut source rows are shown blue (distinct from the grey of multi-select). `Esc` peels back one layer at a time: if a selection was live when `c`/`x` was pressed, the first `Esc` clears the clipboard (keeping the selection) and a second `Esc` clears the selection. The earlier per-row "valid/invalid target" colouring (green/red cursor + dimmed rows) was removed as noise — an incompatible paste simply reports `paste error: …` in the status bar. (2026-06-08)

### Fixed
- Inline editing — changing a value **between a scalar and an array/inline-table** now works (e.g. `5` → `[1, 2]`, or `{ a = 1 }` → `7`). `Replace` now swaps the VALUE's content element generically, covering scalar↔scalar, struct↔struct, **and** scalar↔struct; previously a scalar↔struct type change errored. (`replace` in `cst_edit.rs`.) (2026-06-09)
- Inline editing — a **non-top-level** single-line array / inline table (one nested as an array *element*, e.g. `[[1, 2]]` or `[{ a = 1 }]`) now edits inline instead of jumping to `$EDITOR`. (`edit_target_kind` no longer forces array-element structured values external.) (2026-06-09)
- Clipboard — **cutting a comment and pasting it into an array no longer loses the comment.** Comments live in a table/root's decor, so they can't go inside an array/inline-table/AoT; the paste is now validated *before* the cut's source deletion, so an illegal target aborts non-destructively (clipboard kept, comment intact, status `comments can only go into a table or the document`) — cut behaves like copy. The model `InsertComment` also rejects non-table parents (`MutateError::Illegal`). (2026-06-09)
- Editing — opening an **array-of-tables *group*** node in `$EDITOR` (`E`) no longer shows a blank buffer. It now serializes all of the group's `[[x]]` entries and writes the edit back over the whole group (`aot_group_span` in `cst_edit.rs`); editing a single entry is unchanged. (2026-06-09)
- Clipboard — paste-mode cues refined: **copy** sources stay blue, **cut** sources are now green (distinct background); and the green insertion line now starts at the cursor each time paste mode is (re)entered, instead of wherever the previous paste session left it. (2026-06-09)
- Insert/paste/move — a node can no longer be placed where TOML's table-capture rule would silently re-key it. Inserting a **scalar/array/inline-table after a `[table]`/`[[array]]` header** (it would become a member of that section), or a **`[table]`/`[[array]]` before the keys above it** (it would capture them), is now rejected non-destructively with a status message; the document is left untouched and a failed paste keeps its clipboard. The check is a source-order *partition* gate in `cst_edit::insert` (so it covers paste, move, and `a`/add alike), surfaced as a new `MutateError::Illegal`. (Phase B of cross-layer ops; D1/D5 in the plan.) (2026-06-09)
- Parsing — dotted table **headers** without an explicit parent (`[product_table2.a]` / `[product_table2.b]` with no `[product_table2]`) now nest under an implicit `product_table2` branch, matching `[product_table]`. Projection only flattens implicit tables created by dotted *keys* (`a.b.c = 1`, which toml_edit marks `is_dotted()`); a dotted header is implicit but not dotted, so it projects as a real branch. (2026-06-07)
- Editing — `E` (external `$EDITOR`) on the root/file node no longer fails on save with `operation not supported by this format`. A `Replace` with an empty path now reparses the edited text as the whole document (invalid TOML is rejected and leaves the document untouched). (2026-06-07)
- Editing — opening `$EDITOR` on a structured node (`[table]`, array, inline table, array-of-tables entry) no longer starts with an empty first line: the node's leading blank separator is trimmed from the editor view. The blank line is re-attached on save (`split_leading_blank_lines` in `toml_doc.rs`), so file spacing round-trips unchanged; leading comments are still shown and editable. (2026-06-07)
- Clipboard — a failed paste no longer discards the clipboard. Previously only a key collision preserved it; any other error (e.g. pasting a bare value into a table) silently emptied the clipboard, forcing a re-copy. `do_paste` now restores the remaining fragments on every failure path, so you can move the cursor to a valid target and retry. (2026-06-08)
- Clipboard/Filter — pasting into a key collision and choosing overwrite/rename (`o`/`r`) while a filter is active now inserts at the correct position. The retry path resolved its insertion index from the *visible* (filtered) row list, so it could disagree with the initial paste; it now uses the full-tree `true_sibling_index`, matching paste/add. (2026-06-08)
- Clipboard — cutting a node and pasting it back into the **same scope** no longer fails with "Key '…' already exists". Cut now routes through the atomic `Mutation::Move` (delete-before-reinsert), so a reposition within the same parent is detected as a move rather than a collision; any paste failure also rolls the document back, so the clipboard is never lost. (2026-06-08)

## [v0.3.0] - 2026-06-07

### Changed
- Editing — `E` on an **array-of-tables entry** (`product[0]`) now opens `$EDITOR` with just that single `[[product]]` block (was: the whole array-of-tables). Write-back goes through a new AoT-entry `Replace` branch (`replace_aot_entry`) that rewrites only that entry, preserving the others and the between-entries comments; `edit_node` now truncates the path only at a real `Array` index, keeping AoT-entry indices addressable. (2026-06-07)
- Editing — `e` on a **scalar member of an array-of-tables entry** (`product[0].sku`) now edits inline (and `←/→` nudges, `Tab`→Name renames) instead of opening `$EDITOR` on the whole AoT. `parent_table_mut`/`concrete_table_mut` now descend a `Key→Index` AoT entry; the inline rule keys on the absence of an `Array` ancestor, so array-of-inline-table members (`x = [{ a = 1 }]`) still open `$EDITOR`. (2026-06-07)
- Editing — `e` on a **single-line comment** now edits inline (the raw `#`-prefixed text as the sole field, no `Tab`/name, committed via `Mutation::EditComment`) instead of opening `$EDITOR`. Merged multi-line comments and comments nested in an array-of-tables still open `$EDITOR`. (2026-06-07)
- Editing — `e` on a scalar **member of an inline table** (`pt = { x = 1 }`) now edits inline instead of opening `$EDITOR`; `Tab`→Name renames the key in place (`Mutation::Rename` now handles inline-table keys, preserving order and the other members). (2026-06-07)
- Editing — opening `$EDITOR` on a **structured** node (table/inline table/array/array-of-tables) now carries its adjacent leading comment(s) into the editor, and edits to that comment round-trip on save. Previously only `[table]` headers carried their comment; arrays did not. Scalars (including multiline strings) never carry comments. (2026-06-07)
- Editing — `e` on a multiline string now opens `$EDITOR` instead of the single-line inline editor, matching the existing behavior for nested arrays/tables. Single-line scalars still edit inline. (2026-06-06)
- Editing — `e` on a scalar **element of an array** now edits it inline (was: opened an empty `$EDITOR` and failed to save). Write-back goes through `Replace` on the trailing `Index` path via `Array::replace`, preserving the other elements and their formats. Non-scalar array elements still open `$EDITOR`. (2026-06-06)
- Editing — `←/→` value-nudge now also works on a scalar array element (toggle bool / step int/float in place). (2026-06-06)
- Editing — the value-nudge now re-applies underscore digit grouping when the original value had it (decimal every 3, hex/oct/bin every 4, float fractional every 3), so `1_000_000` stays grouped after a step. (2026-06-06)

### Added
- Editing — the inline editor now supports `Del` (forward-delete the char at the caret, alongside `Backspace`). (2026-06-07)
- Filter — `/` filter is now a full inline text field: a reverse-highlighted caret, `←/→/Home/End` to move it, and `Backspace`/`Del` to edit at the caret (was: append/pop only at the end). (2026-06-07)
- Comments — adjacent comment lines now project as a single multi-line comment node (a blank line, or any non-`#` line, breaks the group), so a comment block is one navigable node. Comment nodes now carry their text as a value, shown in the VALUE column and the detail popup. (2026-06-07)
- Editing — `e`/`E` on a comment now opens `$EDITOR` with the comment's raw `#`-prefixed text and writes the edit back into the decor via a new `Mutation::EditComment` (was: opened an empty editor and could not save). Edited text must remain comment lines, else the document is left untouched. (2026-06-07)
- Editing — `a` on an array now inserts a new element (seeded `""`) and opens it for inline editing, instead of failing with a key-collision/`NotFound`. (2026-06-06)
- Editing — `Tab` in the inline editor toggles between the Value (default) and Name fields; committing a changed Name renames the key via a new position/decor-preserving `Mutation::Rename`. `Tab` is disabled for array elements (no key), and the NAME field gets the same horizontal-overflow scrolling as VALUE. (2026-06-06)
- Editing — scalar elements of nested arrays (array-of-arrays, `Key Index Index…`) now edit inline and nudge in place, addressed via `array_at_mut`. (2026-06-06)
- CI — `.github/workflows/release.yml`: on a `v*.*.*` tag, cross-compiles `confy` for Linux x86_64 (gnu + musl), macOS (arm64 + Intel), and Windows x86_64 + i686 (MSVC), packages tar.gz (Unix) / `.exe` (Windows), emits `SHA256SUMS`, and publishes a GitHub Release (annotated-tag message + auto-generated notes).

### Fixed
- Comments — editing or deleting a standalone comment now works **wherever it sits**, not just before the first item of a container. The shared decor locator (`transform_comment_in_decor`) used to inspect only the first key, so a comment before any *non-first* item — e.g. a section-separator above `[[products]]` when an earlier section precedes it — silently failed to save or delete. It now sweeps every comment-bearing slot (`sweep_table_comment_slots`: each key's `leaf_decor`, each `[table]` header decor, each array-of-tables entry prefix, and the document trailing), stopping at the first slot that matches. This also covers comments **between** AoT entries and **inside** an AoT entry. (2026-06-07)
- Comments — a comment **inside an array-of-tables entry** (`[[product]]` / `#123` / `name = …`) now edits inline like any other single-line comment, and `E` opens `$EDITOR` with its text instead of a blank buffer. Its path carries an `Index` (the entry), but it is decor-addressable, so editing keys on the absence of an `Array` ancestor (shared `no_array_ancestor`) rather than the mere presence of an `Index`. (2026-06-07)
- Editing — deleting a standalone comment node no longer fails with `delete error: path not found`; `Delete` now strips the comment from its decor slot (like `uncomment`) instead of trying to remove a non-existent `#comment:N` table key. (2026-06-07)
- TUI — multi-line cell values (merged comments, multiline strings, and elements of a multiline-formatted array) now render a single-line preview (first line + ` …`) in the VALUE column. Previously a multiline-array element showed nothing because its repr carries leading newline+indent decor; the full text remains available in the detail popup. (2026-06-07)
- TUI — the main tree viewport now persists its scroll offset across frames, so the cursor moves within the visible window instead of staying pinned to the bottom edge and scrolling on every key. (2026-06-06)
- Editing — replacing a value no longer drops a standalone `#` comment sitting above its key; `Replace`/`Insert` overwrite now updates the value in place (preserving key decor) instead of re-inserting the key. (2026-06-06)
- Editing — `e` on a node nested inside an array/AoT (or on an element of a multiline array) no longer opens an empty editor; it edits the nearest addressable container, and multiline-array string elements edit inline with their indentation preserved. (2026-06-06)
- Move — moving a node into a table no longer drops the leading comments and blank lines above it; the move now carries the key's `leaf_decor` (capturing `(Key, Item)` and re-inserting via `entry_format`) instead of re-serializing through a fresh document. Array destinations are unchanged. (2026-06-06)

## [v0.2.0] - 2026-06-06

Single-file TOML editor with a CST-projection architecture: tree navigation/selection/editing,
byte-identical round-trip preservation, undo/redo, fuzzy filter, an inline value editor, a
read-only scalar/branch Format axis, and a scrollable detail popup.

### Added
- Core MVP — single-file TOML editor on a CST projection (`toml_edit::DocumentMut` as the source of truth), tree navigation/selection, Insert/Delete/Replace/Move/Remark mutations, undo/redo, fuzzy filter, `$EDITOR` integration, byte-identical round-trip. (2026-05-27 … 05-28)
- wenv-style title bar + columnar tree — `confy — <file> ──── v<version>` header and a `NAME / TYPE / VALUE` ratatui `Table` so type and value align in fixed columns. (2026-06-03)
- Scalar `Format` attribute — read-only writing style derived during projection (integers: dec/hex/oct/bin; strings: basic/literal/multiline). `ScalarType::Datetime` split into the four TOML datetime types (offset/local-datetime/local-date/local-time).
- Inline editor (`Mode::Edit`) — `e` edits a plain scalar in place with a not-enforced type check (confirm-on-change prompt) and falls back to `$EDITOR` for nested arrays/tables; `E` forces `$EDITOR` on any node. Cursor shown by reverse-highlight (no glyph drift), `Home`/`End` support, horizontal scroll on overflow with a `⟨start–end/len⟩` hint, and semantic-error feedback in the status line.
- `←`/`→` value nudge — toggle a bool or step an integer/float by ±1, preserving base and decimal precision.
- `a` Add node — inserts `new_field = ""` below the cursor and opens the inline editor. (Clear-to-null / `Del`-clear intentionally omitted — TOML has no null.)
- `i` detail/info popup on any node — content-adaptive height (`[5, 80% of screen]`), scrollable (`↑`/`↓`/`j`/`k`, `PgUp`/`PgDn`, `Home`/`End`), shows the full wrapped value and a `Format` line for every node (branch detail splits Type vs. writing style, e.g. inline table → Type table / Format inline).

### Changed
- `n` (New node via `$EDITOR`) replaced by `a` (Add node, inline).
- `TYPE` tree column became `TYPE/FORMAT`; inline tables read `table/inline`, standard tables stay `table`.
- Inline-editor horizontal viewport is persistent state, clamped minimally per keystroke.

### Fixed
- Inline-commit semantic-check errors are shown in the status line instead of being hidden behind the edit-mode hint.
- Inline-edit viewport no longer pins the cursor to the right edge when moving left after reaching the end.
- Replace/Remark review fixes — preserve key position, canonical serialization (no double-space), nested/AoT comment round-trip.
