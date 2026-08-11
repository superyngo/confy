# Code Audit Report — confy

Date: 2026-08-11
Scope: `.worktrees/json-schema-support` (clean, 57 commits ahead of `main`, most current tree — audited this, not `main`'s dirty WIP). Rust workspace (5 crates, 38.1k LOC) + web frontend (22 TS files, 7.5k LOC). 6 parallel scouts + direct verification (`cargo clippy`, `tsc --noEmit`, `cargo test -p confy-core`, `cargo tree --duplicates`, `git ls-files`).

Audit lens (per user request): **optimization, organization, integration, cleanliness** — not the code-auditor skill's default six-category sweep. A "Testing" note is included where it surfaced material findings adjacent to these axes, but testing was not independently swept.

## Executive Summary

- **Health: good bones, one systemic perf gap.** `cargo clippy --workspace`: 1 warning total. `tsc --noEmit`: clean. `cargo test -p confy-core`: 472/472 passing. No secrets in CI, no tracked debug artifacts, no dead-code suppressions of consequence. The codebase is unusually clean for its size — the audit's real findings are architectural, not hygiene.
- **Top 3 priorities:**
  1. **[Optimization, Critical]** Every single mutation — including one keystroke's inline-edit commit — re-serializes the whole document, re-projects the whole tree, and unconditionally re-runs full JSON-Schema validation, synchronously, with no dirty-region tracking (`session.rs:2501-2515`, `session.rs:1379-1391`). This is fine today; it's a perf cliff waiting for a user with a large document + nontrivial schema.
  2. **[Integration, High]** TUI bypasses `Session::dispatch()` entirely and calls ~40 raw `Session` methods directly, so cross-cutting dispatch logic (shift-select reset, branch/leaf toggle decision) is hand-duplicated in `confy-tui/src/tui/mod.rs`, confirmed to still match today but structurally guaranteed to drift eventually.
  3. **[Organization, High]** Five files exceed 5–9x the project's own 800-line threshold as single-responsibility units: `cst_edit.rs` (7064 ln), `session.rs` (3668 ln), `yaml/edit.rs` (3958 ln), `web/ui.ts` (1856 ln), `web/touch/app.ts` (1442 ln). Each mixes 4-6 unrelated concerns with no internal module split.

## Findings by Category

### Optimization

#### 🔴 Critical
- **`crates/confy-core/src/session/session.rs:2501-2515`** (`on_mutation_success`) — verified directly. Runs after *every* successful mutation: full `doc.serialize()`, full `doc.project()`, unbounded `history.push(full_snapshot)`, then unconditional `revalidate_schema()`. No size guard, no debounce, no dirty-region tracking. Every keystroke-driven commit re-does the entire pipeline regardless of document size.

#### 🟠 High
- **`crates/confy-core/src/session/session.rs:1379-1391`** (`revalidate_schema`) — called unconditionally from `on_mutation_success`, `apply_schema_text`, and undo/redo. Full `to_value()` + full `value_bridge::bridge()` tree walk + full `jsonschema::Validator::iter_errors` pass, synchronous, on the edit thread, every commit. Perf-cliff risk scales with document size × schema complexity (`$ref`/`allOf`).
- **`crates/confy-core/src/session/session.rs`** (~14+ call sites: 1150, 1168, 1183, 1196, 1207, 1543, 1621, 2227, 2307, 2412, 2462, 2913, 3107, 3233, 3240) — `self.visible_rows()` does a full O(n) tree flatten + per-row clone, called only to `.find()` one row matching the cursor path. Point-lookup paying full-materialization cost on inline-edit-entry, rename, nudge, delete, paste-target-resolution hot paths.
- **`web/render.ts:184` / `web/ui.ts` render path** — `renderTree()` does `treeEl.innerHTML = rows.map(renderRow).join('')` unconditionally on every `send()`/`batch()` — i.e. every cursor move, not just structural edits. No row-level diffing/virtualization. Mitigated for text edits (native `<input>`, commits once), not for navigation.
- **`crates/confy-tui/src/tui/ui.rs:259-395`** (`draw_tree`) — rebuilds a full `Vec<Row>` for *all* logical rows every `terminal.draw()` call, not the visible viewport slice, despite `TableState` offset already tracked. Wasted work scales with total expanded rows, not screen height.

#### 🟡 Medium
- **`crates/confy-core/src/session/state.rs:211-228`** (`History`) — `push()` stores full serialized document text per mutation, no cap, no ring-buffer, no diff storage. Unbounded memory growth = edit count × document size.
- **`crates/confy-ffi/src/lib.rs:116-121`** (`external_edit`) — calls full `session.snapshot()` (rebuilds entire `Vec<ViewRow>` via `visible_rows()`) just to read one `Option` field, discarding the rest, on every poll.
- **`.github/workflows/publish-vscode.yml:43-45`** — `actions/setup-node@v4` has no `cache: 'npm'`, unlike the same workflow's Rust side (`Swatinem/rust-cache@v2`). `release.yml`'s Windows job has the same gap via `cf-build.sh`'s uncached `npm install`.
- **`cargo tree --duplicates`** — confy-tui binary compiles 3 major versions of `getrandom` (0.2/0.3/0.4) + 2 each of `itertools`/`hashbrown`, all transitively forced by upstream crates (not directly fixable by a confy pin). Build-time/binary-size cost, not a correctness issue.

#### ⚪ Low
- **`crates/confy-tui/src/tui/mod.rs:126-206`** — `wrapped_line_count` recomputed on every scroll keypress *and* again inside the draw call for the same (text, width) — no memoization. Low severity, small text.

### Organization

#### 🔴 Critical
- **`crates/confy-core/src/model/cst_edit.rs`** — 7064 lines (~4170 non-test), the largest file in the repo by ~2x. Mixes TOML splice-mutation dispatch, string-escape codecs, scalar-notation conversion (~700 ln), dotted-table algebra, AoT-group algebra, and move/paste splicing in one file. `json/edit.rs`/`yaml/edit.rs` are already split from their `project.rs`/`doc.rs` siblings — this file wasn't.
- **`web/ui.ts`** — 1856 lines, 6+ unmixed concerns: boot/bootstrap, full render orchestration, keyboard dispatch (~250 ln), VS Code host bridge (~150 ln, fully `VSHOST`-gated so cleanly extractable), save/open/convert glue, pointer/menu/marquee wiring. `render.ts`/`select.ts`/`dnd.ts`/`menu.ts`/`host-io.ts` are already carved out for adjacent concerns — the orchestrator absorbed everything else that wasn't.

#### 🟠 High
- **`crates/confy-core/src/session/session.rs`** — 3668 lines, one `impl Session` block covering cursor/selection state, inline-edit-buffer lifecycle, schema-hint clamping, clipboard cut/copy/paste (with its own collision-prompt sub-state-machine), undo/redo, and i18n status formatting. Sibling files (`dispatch.rs`, `view.rs`, `selection.rs`, `insertion.rs`) already carry narrower responsibilities — this is the natural next split.
- **`crates/confy-core/src/model/yaml/edit.rs`** — 3958 lines: mutation dispatcher, indent engine, path resolver, opaque-node guard all together. Crosses the 800-line threshold ~5x.
- **`web/touch/app.ts`** — 1442 lines: app-shell HTML string-building, module-level UI state, ~10 bottom-sheet controllers, gesture handling (tap/swipe/reorder/sheet-drag/splitter-drag), file I/O orchestration, and boot, with no sub-module extraction beyond comment banners. Plays the role `ui.ts` + `menu.ts` + others jointly play on desktop, compressed into one file.
- **`crates/confy-tui/src/tui/ui.rs`** — non-test render code ~939 lines: tree table + 7 distinct overlay renderers (detail/help/type-filter/kind-switch/convert/lang-picker/prompt) + shared text-layout helpers in one file, unlike the otherwise-granular `keys.rs`/`search.rs`/`selection.rs`/`type_filter.rs` sibling files.

#### 🟡 Medium
- **`crates/confy-core/src/model/json/edit.rs`** — 2116 lines, smallest of the three format backends but still ~2.6x the threshold.

### Integration

#### 🟠 High
- **`crates/confy-core/src/session/dispatch.rs` vs `crates/confy-tui/src/tui/mod.rs:322-327, 344-364`** — TUI never calls `Session::dispatch(Intent)`; it calls ~40 raw `Session` methods directly (documented in `dispatch.rs`'s own doc comment, PORTING §8.4, as an intentional-but-fragile split). Two cross-cutting behaviors are hand-duplicated as a result: the shift-select-round reset rule, and the `Intent::ToggleExpand` branch-vs-leaf decision. Both verified to currently match, but any future change to one copy silently desyncs TUI vs Web/desktop behavior — this is the exact class of bug the recent PageUp/PageDown work in this session just fixed for a different key-handling gap.
- **`web/render.ts:105-181` vs `web/touch/render.ts:66-113`** — `renderRow()` (desktop) and `rowHTML()` (touch) independently build near-identical per-row HTML (indent/caret, key span, value cell, kind/type badge, trailing comment, positional styling) as two hand-maintained ~50-80 line string builders. Everything else at this layer (`host-io.ts`, `panel.ts`, `samples.ts`, `convert-dialog.ts`, `help-content.ts`, `toolbar-fold.ts`) *is* correctly unified and consistently imported by both `ui.ts` and `touch/app.ts` — this is the one place the established shared-module pattern wasn't followed. A field/badge added to `ViewRow` must be hand-mirrored in both files today.

#### 🟡 Medium
- **`.github/workflows/release.yml` (~200-215) vs `publish-vscode.yml` (~25-46)** — near-identical "install Rust+wasm32, rust-cache, cargo-binstall wasm-pack, run `cf-build.sh`" step sequences duplicated verbatim across two workflow files rather than a reusable composite action — concretely already drifted (only one of the two caches npm, see Optimization/Medium above).
- **`crates/tauri-plugin-confy-picker/Cargo.toml:7-9,12`** — declares `thiserror = "2"` directly instead of `thiserror.workspace = true` (pinned "1" everywhere else), and pins `tauri = "2.11.3"` exactly while `confy-tauri/Cargo.toml:23` uses the loose `tauri = "2"`. Both cause avoidable duplicate dependency compilation and inconsistent upgrade cadence between confy's own sibling crates.
- **`editors/vscode/package.json` vs `web/package.json`** — esbuild `^0.24.0` vs `^0.25.0`, typescript `^5.5.0` vs `^5.6.0`. Same tools, drifted ranges between the repo's two JS toolchains.

#### ⚪ Low (acknowledged, not silent drift)
- **`web/ui.ts` vs `web/touch/app.ts`** — `send()`, `batch()`, `modeTag()`, `openText()`, `openSample()`, `setRawView()`, `chooseLang()` are re-implemented per-file, but several already carry explicit "mirrors ui.ts" comments acknowledging the duplication is intentional (each host's render/DOM shell genuinely differs). Lower priority than the row-builder duplication above since these are 5-15 line functions, not ~80-line builders.

### Cleanliness

#### 🟡 Medium
- **`web/touch/app.ts:1160-1183`** (`openOpenedUrl`) — two `console.log('[confy] opened url/name:'...)` calls behind a comment: `// TEMP (M1 Task 3 device debugging): ... remove once the content:// read bug is diagnosed.` Never removed. Confirm the bug is actually resolved, then delete.
- **`editors/vscode/build.mjs:1-3`** — top-of-file comment admits esbuild deadlocks bundling from the `/Volumes/Home` volume path, worked around by a scratch-copy convention rather than fixed, with no defensive check in the script itself if run from the deadlocking path.

#### ⚪ Low
- **`crates/confy-core/src/model/json/project.rs:8-18`** — `#[allow(dead_code)]` on `pub(crate) enum Target`; variants appear used elsewhere in the file. Suppression looks stale — worth confirming which single variant is actually unused and removing just that, rather than the blanket allow.
- **`crates/confy-core/src/model/yaml/parse.rs:20-23`** — `#[allow(dead_code)]` on `parse()`, paired with a header docblock still calling itself a "SPIKE... prove the gate, not ship production code" — but the function is the real production entry point called by `yaml/doc.rs`. Stale doc/attribute framing left over from an earlier gate-spike phase.
- **`crates/confy-tauri/src/lib.rs:77-79,106-108`** — two `Mutex::lock().unwrap()` in live command/event handlers; a poisoned mutex would crash the whole desktop app rather than degrade, in a crate whose own doc comment stresses staying "a thin, robust shell."
- **1 clippy warning** — `crates/confy-core/src/session/session.rs:1578` — `collapsible_match`, trivially fixable.

## Testing note (surfaced during audit, adjacent to Optimization/Integration)

Not requested as a fifth axis, but load-bearing enough to flag: `crates/confy-ffi/functional_smoke.mjs` (the only proof the wasm `Intent`→`SessionSnapshot` wire contract works) has **zero coverage of the JSON Schema feature** despite `confy-core`'s `schema/` module being thoroughly tested at the core layer (`schema_headless.rs`, 788 lines) — nothing proves `SchemaLoaded`/`SetSchema`/`SchemaEnum` actually round-trip through `serde-wasm-bindgen` the way the rest of the Intent surface does. Separately, the same script carries one long-standing, explicitly-waived failing assertion (`grid active after toggle`, documented since 2026-08-06, never fixed) inside a script that gates merges by convention — a de-facto permanently-skipped check hiding inside a pass/fail script.

## Prioritized Action Plan

**Quick wins (< 1 day)**
1. Fix the 1 clippy `collapsible_match` warning (`session.rs:1578`).
2. Remove the two `TEMP` `console.log`s in `web/touch/app.ts:1160-1183` (confirm the underlying bug first).
3. `tauri-plugin-confy-picker/Cargo.toml`: switch `thiserror = "2"` → `thiserror.workspace = true`, loosen the exact `tauri = "2.11.3"` pin to match `confy-tauri`'s `"2"`.
4. Add `cache: 'npm'` to `publish-vscode.yml`'s `setup-node` step.
5. Drop/repoint the stale `#[allow(dead_code)]` + "SPIKE" framing in `yaml/parse.rs:20-23` and the one genuinely-unused `Target` variant in `json/project.rs:8-18`.
6. Align `editors/vscode/package.json`'s esbuild/typescript ranges with `web/package.json`'s.

**Medium-term (1-5 days)**
1. Extract a shared row-anatomy builder consumed by both `web/render.ts` and `web/touch/render.ts` (mirrors the existing `panel.ts`/`host-io.ts` pattern already proven to work across both hosts).
2. Give `Session` an O(1) cursor→row lookup (indexed by path, or a `find`-short-circuiting variant of `visible_rows`) and replace the ~14 call sites in `session.rs` that currently pay full-materialization cost for a point query.
3. Extract `.github`'s duplicated "build wasm+web frontend" step sequence into a composite action; fix the npm-cache gap as part of that extraction rather than separately.
4. Split `confy-tui/src/tui/ui.rs`'s 7 overlay renderers into per-overlay files, matching the crate's existing granular module convention.
5. Give `crates/confy-tui/tests/schema_io.rs` a mock-server test for the `SchemaSource::Url` fetch path (currently zero coverage).
6. Add JSON Schema scenarios to `functional_smoke.mjs`, or accept and document the coverage gap explicitly; separately, either fix or formally document-and-track the long-standing `grid active after toggle` failure instead of leaving it as tribal knowledge in a plan doc.

**Long-term (> 5 days)**
1. Route TUI through `Session::dispatch(Intent)` instead of raw method calls, eliminating the hand-duplicated cross-cutting logic in `confy-tui/src/tui/mod.rs` — the structural fix behind this audit's top Integration finding.
2. Add dirty-region tracking (or at minimum a cheap "did the schema-relevant subtree change" check) to `on_mutation_success`/`revalidate_schema` so validation cost stops scaling with total document size on every keystroke; pair with a capped/diffed undo history instead of full-text snapshots per edit.
3. Split `cst_edit.rs` (7064 ln) into per-concern modules (escape codecs / scalar-notation conversion / dotted-table algebra / AoT-group algebra / move-paste), and do the equivalent split for `session.rs` (3668 ln) and `yaml/edit.rs` (3958 ln).
4. Add row-level/keyed diffing (or basic virtualization) to `web/render.ts`'s `renderTree()` and `confy-tui`'s `draw_tree`, replacing whole-tree `innerHTML`/`Vec<Row>` rebuilds on every navigation keystroke with viewport-scoped updates.

## Metrics

- Files analyzed: ~120 (crates/*/src + crates/*/tests + web/*.ts + web/touch/*.ts + CI workflows + build scripts + Cargo/package manifests)
- Lines of code: 38,101 Rust (crates/**/*.rs) + 7,462 TypeScript (web/**/*.ts, excl. node_modules)
- `cargo clippy --workspace`: 1 warning
- `tsc --noEmit`: 0 errors
- `cargo test -p confy-core`: 472 passed, 0 failed, 0 ignored
- Files over the 800-line organizational threshold: 8 (`cst_edit.rs` 7064, `yaml/edit.rs` 3958, `session.rs` 3668, `json/edit.rs` 2116, `ui.ts` 1856, `touch/app.ts` 1442, `ui.rs` (confy-tui, non-test) ~939, `session_headless.rs` test file 1423)
- Tracked stray artifacts (`.log`/`.DS_Store`/tmp): 0
- Hardcoded secrets in CI: 0
