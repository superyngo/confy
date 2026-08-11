# Audit Remediation — Implementation Plan

Date: 2026-08-11
Source: `docs/superpowers/audits/2026-08-11-optimization-organization-integration-cleanliness-audit.md`
Scope: full backlog (Quick wins + Medium-term + Long-term), phased. 16 items.

## Overview

Turns the audit's Prioritized Action Plan into discrete, file-referenced tasks. Grouped into
three phases matching the audit's own risk/size tiers. Each phase is a checkpoint: land and
verify Phase 1 before starting Phase 2, etc. — a mechanical cleanup regressing because a
concurrent architectural refactor was mid-flight is the failure mode to avoid.

## Architecture Decisions

Confirmed via a `/grilling` + `/domain-modeling` session on 2026-08-11 — see
`docs/adr/0003-audit-remediation-undo-cap-and-tui-dispatch-boundary.md` for the two decisions
that crossed the ADR bar. Scope is confirmed at exactly the audit's own 16-item Prioritized
Action Plan: `web/ui.ts`, `web/touch/app.ts`, and `json/edit.rs` were flagged as oversized in the
audit's Organization findings but deliberately left out of its own action plan, and stay out of
this plan too.

- **Schema dirty-check (Task 14) uses a per-schema "is this schema fully analyzable" flag, not a
  precomputed constrained-path set.** Grilled correction: `edit_hint()`/`hints_edit.rs` has no
  cache to reuse — it re-walks the raw schema from scratch on every call, and
  `value_bridge::PointerMap` maps document structure, not schema constraints. The corrected
  design: at `apply_schema_text` time, walk the raw schema once to record whether it uses only
  same-document `$ref`/`properties`/`items` (no remote `$ref`, `allOf`/`not`/`if-then-else`, or
  `oneOf`/`anyOf` beyond the existing `const` carve-out) anywhere — cache that single bool on
  `SchemaState`. If true, a new per-mutation path-walk (sibling function to
  `hints_edit::resolve_subschema`, sharing its `deref` helper, in a new module — not folded into
  `hints_edit.rs`, whose `None`-on-decline polarity means the opposite thing there) gives an exact
  O(schema depth) "does this path carry a constraint" check per mutation, skipping
  `revalidate_schema()`'s full walk when the answer is no. If false, Task 14's optimization is a
  no-op for that schema — always revalidate, identical to today's behavior, zero risk of a stale
  violation. See `docs/adr/0003-audit-remediation-undo-cap-and-tui-dispatch-boundary.md`.
- **History cap (bundled into Task 14) uses a fixed-size ring buffer**, not compressed/diffed
  snapshots — matches the existing full-text-snapshot design, capped at a fixed 200 entries, not
  configurable (see ADR 0003 — no host exposes undo depth as a setting, and none should).
- **Session O(1) cursor→row lookup (Task 9) is a `HashMap<Path, usize>` cache**, invalidated
  whenever `self.tree` is rebuilt (same point `visible_rows()`'s backing data changes today) —
  not a persistent incremental index. Simplest thing that removes the O(n) `.find()` from the
  ~14 hot call sites.
- **cst_edit.rs / session.rs / yaml/edit.rs splits (Task 15) are pure code motion** — `mod.rs`
  + sibling files, `pub(crate)` where cross-module, no behavior change, no re-architecture of
  the algorithms themselves. Boundaries follow the seams the audit already identified (each
  file's own doc comments already name its distinct concerns).
- **TUI → `dispatch(Intent)` routing (Task 13) is the single highest-risk task in this plan.**
  495 `session.`-qualified references exist in the TUI crate; most are read-only state queries
  (`app.session.mode`, `.tree`, `.cursor`, …) that `dispatch()` itself also does internally and
  are **not in scope** — only the ~40 *mutating* method calls the audit flagged (`schema_enum_move`,
  `schema_enum_jump`, `kind_switch_move`, selection/cursor/edit mutators, etc.) route through
  `Intent` instead. Coverage check: `crates/confy-core/src/session/intent.rs`'s `Intent` enum
  already models nearly every one of these (added incrementally as the Web UI contract grew) —
  confirmed by inspection, e.g. `SchemaEnumMove`/`SchemaEnumJump`/`KindSwitchMove` all exist
  already. A handful of TUI-only presentation states (language picker, kind-switch popup nav)
  are **App-level, not Session-level**, and stay as direct calls — they're not cross-cutting
  Session behavior, so routing them through `Intent` would add a wasm-wire-format variant no
  other host needs. Grilled and confirmed: scope stays at ~40 mutating calls (not deferred, not
  shrunk further) despite the audit's ~40 estimate undercounting total references ~12x — the
  read-only/mutating split is what makes that gap irrelevant. Runs **last**, checkpointed by
  risk, not uniformly: groups 1-4 (navigation/selection/filter/kind-switch+convert — read-heavy,
  no mutation-commit or edit-buffer interaction) verify together in one batched checkpoint;
  groups 5-8 (schema-enum/inline-edit/mutations+undo/lifecycle — touch mutation-commit or
  edit-buffer state) each get their own individual build+test+manual-smoke checkpoint via the
  existing `hub`-driven TUI smoke-test pattern before moving to the next.

## Phase 1 — Quick wins (< 1 day)

### Task 1: Fix clippy `collapsible_match`
- **File**: `crates/confy-core/src/session/session.rs:1578`
- **Description**: `if let crate::schema::EditHint::Enum(options) = hint { if !options.is_empty() { ... } }`
  collapses to `if let crate::schema::EditHint::Enum(options) = hint && !options.is_empty() { ... }`
  (or nested `if let` chain per clippy's suggested fix) — single-arm match nested directly inside
  another single-arm `if let`.
- **Dependencies**: None.
- **Verify**: `cargo clippy --workspace` → 0 warnings.

### Task 2: Remove TEMP debug `console.log`s
- **File**: `web/touch/app.ts:1250-1270` (`openOpenedUrl`)
- **Description**: Two `console.log` calls (line 1256: opened URL; lines 1262-1269: opened name +
  text length/head) behind a comment (lines 1253-1255) marked "remove once the content:// read bug
  is diagnosed." Confirm the bug is actually resolved first (check CHANGELOG.md for a fix entry
  referencing Android `content://` reads / M1 Task 3; if unresolved, downgrade this task to
  "leave in place, drop the stale comment" instead of deleting).
- **Dependencies**: None.
- **Verify**: `grep -n "console.log" web/touch/app.ts` no longer matches this function; `tsc --noEmit` clean.

### Task 3: Align dependency pins in `tauri-plugin-confy-picker`
- **File**: `crates/tauri-plugin-confy-picker/Cargo.toml:12,14`
- **Description**: `thiserror = "2"` → `thiserror.workspace = true` (workspace pins `"1"`
  everywhere else — check `Cargo.toml` root `[workspace.dependencies]` for the exact pinned
  version first, since this crate is currently the odd one out at major version 2, not just a
  loose-vs-exact difference). `tauri = { version = "2.11.3" }` → `tauri = { version = "2" }` to
  match `confy-tauri/Cargo.toml:30`'s loose pin.
- **Dependencies**: None.
- **Verify**: `cargo build -p tauri-plugin-confy-picker` succeeds; `cargo tree --duplicates` shows
  no new duplicates introduced (confirm `thiserror` major-version alignment doesn't break the
  plugin's own error-handling code — `thiserror` 1→2 has a small breaking surface, e.g. `#[error(transparent)]`
  interactions; grep the crate's `src/` for `thiserror::Error` derives and check they still compile).

### Task 4: Cache npm in `publish-vscode.yml`
- **File**: `.github/workflows/publish-vscode.yml:43-45`
- **Description**: Add `cache: 'npm'` + `cache-dependency-path` to the `actions/setup-node@v4`
  step (mirror whatever `release.yml`'s Windows job / other Node-caching workflow already does,
  if any exists — else the direct `cache: 'npm'` with `cache-dependency-path: editors/vscode/package-lock.json`).
- **Dependencies**: None.
- **Verify**: Workflow YAML is valid (`actionlint` if available, else manual review); cannot
  functionally verify without a live GitHub Actions run — note this in the PR/commit message.

### Task 5: Remove stale `#[allow(dead_code)]` framing
- **Files**:
  - `crates/confy-core/src/model/yaml/parse.rs:1-22` — the module docblock still frames itself as
    "SPIKE: ... prove the gate, not ship production code" and `parse()` carries `#[allow(dead_code)]`,
    but `yaml/doc.rs` calls it as the real production entry point. Rewrite the docblock to describe
    current production behavior (keep the useful lexer/parser architecture notes, drop "SPIKE"/"prove
    the gate" framing); remove the now-incorrect `#[allow(dead_code)]`.
  - `crates/confy-core/src/model/json/project.rs:9` — `#[allow(dead_code)]` on `pub(crate) enum Target`.
    First confirm which variant (if any) is genuinely unused: `grep -rn "Target::" crates/confy-core/src/model/json/` and
    cross-reference against the 4 variants (`Member`, `Element`, `Comment`, `Block`). Remove the
    blanket `#[allow(dead_code)]`; if one variant truly has no live reader, either wire it in or
    scope the allow to just that variant with a comment explaining why.
- **Dependencies**: None.
- **Verify**: `cargo build -p confy-core` with the allows removed — 0 new dead-code warnings (or
  exactly the ones intentionally re-scoped); `cargo test -p confy-core` still 472/472.

### Task 6: Align vscode/web JS toolchain versions
- **Files**: `editors/vscode/package.json:130-131` (esbuild `^0.24.0`, typescript `^5.5.0`) vs
  `web/package.json:15-16` (esbuild `^0.25.0`, typescript `^5.6.0`)
- **Description**: Bump `editors/vscode/package.json`'s devDependencies to match `web/package.json`'s
  (the newer pair) — `esbuild ^0.25.0`, `typescript ^5.6.0`.
- **Dependencies**: None.
- **Verify**: `cd editors/vscode && npm install && npm run check` (tsc --noEmit) and `npm run build`
  (esbuild) both succeed with no new errors.

**Phase 1 testing strategy**: no new tests needed — these are mechanical/config fixes covered by
existing `cargo test --workspace`, `cargo clippy --workspace`, `tsc --noEmit` gates. Run all three
after Phase 1 lands.

## Phase 2 — Medium-term (1-5 days)

### Task 7: Extract shared row-anatomy builder
- **Files**: `web/render.ts:111-189` (`renderRow`) and `web/touch/render.ts:66-114` (`rowHTML`) →
  new `web/row-builder.ts` (or extend existing `web/kind-labels.ts` if the two module's imports
  already converge there).
- **Description**: Both functions independently build near-identical per-row HTML (indent/caret,
  key span, value cell, kind/type badge, trailing comment, positional styling) from the same
  `ViewRow` shape. Factor the shared anatomy into one function parameterized over the two hosts'
  divergent bits (desktop: drag-grip + hover `+`/`⋮` buttons + `EditView`/schema-enum-inline
  rendering via `renderValue`; touch: swipe actions + `IC` icon set). Follow the existing
  `panel.ts`/`host-io.ts` pattern already proven to work across both hosts (shared module,
  imported by both `ui.ts`/`render.ts` and `touch/app.ts`/`touch/render.ts`).
- **Dependencies**: None.
- **Integration point**: A field/badge added to `ViewRow` today must be hand-mirrored in both
  files — this extraction is exactly what removes that maintenance tax going forward.
- **Verify**: Existing behavior byte-identical — no visual/DOM diff for either host. Manual
  browser check (desktop `web/index.html` + touch `web/touch.html?ui=touch`): open a sample doc,
  confirm every row renders identically to before (indent, badges, comments, positional `—`
  placeholders) on both surfaces. `tsc --noEmit` clean.

### Task 8: CI composite action for wasm+web build
- **Files**: `.github/workflows/release.yml` (~185-206) and `.github/workflows/publish-vscode.yml`
  (~30-49) → new `.github/actions/build-web-frontend/action.yml`
- **Description**: Both workflows duplicate "install Rust+wasm32 target, `Swatinem/rust-cache@v2`,
  `cargo-binstall` wasm-pack, `actions/setup-node@v4`, run `web/cf-build.sh`" nearly verbatim.
  Extract into a composite action; fix the Task 4 npm-cache gap as part of this extraction (one
  place to get right) rather than patching both workflows separately.
- **Dependencies**: Supersedes Task 4 if done together — if Task 4 already landed in Phase 1,
  this task just consolidates the now-fixed step into the composite action.
- **Verify**: Same as Task 4 — YAML validity + manual review (cannot run a live release build
  in this environment). Diff the two workflows' pre/post step sequences to confirm the
  composite action's inlined behavior is identical to what was removed.

### Task 9: O(depth) cursor→row lookup in `Session` — IMPLEMENTED, corrected from the literal sketch
- **File**: `crates/confy-core/src/session/session.rs` — `visible_rows`, plus new `to_view_row`/
  `is_path_visible`/`view_row_at`/`cursor_row`, plus ~10 migrated call sites.
- **What shipped, and why it differs from the original sketch**: the original text called for a
  `row_index: HashMap<Path, usize>` cache. Tracing the actual costs while implementing showed
  that design was hollow: `visible_rows()`'s expense is the `NodeTree::flatten` walk *and* the
  per-row `ViewRow` field-cloning/violation-filtering in its `.map()` — a bare index map still
  requires materializing the full row list to turn an index back into a `ViewRow`, so it wouldn't
  have saved anything. Worse, its proposed invalidation scope (only `self.tree` reassignment)
  ignored that `self.expanded`/`self.filtered_paths` also change `visible_rows()`'s output at
  ~12 additional call sites (`toggle_expand`, `collapse_all`, `expand_all`, `set_filter`, …) —
  missing any one would have been a silent stale-lookup bug.
  Shipped design instead: `NodeTree::node_at(path)` already resolves a path in O(depth) (walks
  path segments, not the whole tree) — provably cheaper than building any index. `to_view_row`
  factors `visible_rows()`'s per-row projection into a shared helper; `is_path_visible` replicates
  `flatten`'s expand/filter descent gate in O(depth) (checking every ancestor prefix); `view_row_at`
  composes both for a stateless, cache-free O(depth) single-path lookup — `cursor_row()` is the
  `self.cursor` convenience. No persistent cache, so no invalidation surface at all: it's simply
  cheap on every call, correct by construction, matching `visible_rows()`'s exact semantics
  (visibility included) rather than a raw unchecked tree lookup.
  Migrated ~10 single-row call sites (`cursor_is_read_only`, `edit_target_kind`,
  `begin_inline_edit_impl`, `begin_inline_rename`, `nudge`, `add_node_impl` ×2,
  `add_comment_sibling`, `remark`, the `Collision` paste-prompt handler, `selected_paths`) plus
  the pre-existing `cursor_row_path()` (hot: called 3× per `dispatch()` on the WASM/web path).
  Left untouched: `extend_select_up`/`extend_select_down` (genuinely need the full ordered path
  list for `extend_round_to`), `compute_rows` (needs the full list to snap the cursor),
  `cursor_row_index`/`visible_paths` (position-in-order / all-paths queries — no single-path
  shortcut applies), and the 3 `#[cfg(test)]`-only helpers (`select_row`/`row_path`/`visible_keys`).
- **Dependencies**: None (see the corrected note above — Task 14 doesn't touch this code path).
- **Verify**: `cargo test -p confy-core` — `session_headless.rs` 77→80 (3 new tests: direct-lookup
  vs. full-scan parity, no-staleness across a mutation, correct `None` on a collapsed path);
  `cargo clippy --workspace` 0 warnings. `tsc --noEmit` unaffected (core-only change).

### Task 10: Split TUI overlay renderers — IMPLEMENTED, layout corrected
- **File**: `crates/confy-tui/src/tui/ui.rs` (939 non-test lines) → 7 new flat sibling files
  (**not** a nested `overlays/` directory — corrected during implementation: every existing split
  in this crate, `keys.rs`/`search.rs`/`selection.rs`/`type_filter.rs`, is a flat file directly
  under `tui/`; there is no nested-module precedent anywhere in the crate, so a new `overlays/`
  subdirectory would itself have been the convention deviation, not the fix for one):
  `overlay_detail.rs` (`draw_detail_overlay`, `detail_popup_rect`, `detail_full_text`,
  `wrapped_line_count`), `overlay_help.rs` (`draw_help_overlay`), `overlay_type_filter.rs`
  (`draw_type_filter_overlay`, `type_filter_inner_height`, `type_filter_page_step`),
  `overlay_kind_switch.rs` (`draw_kind_switch_overlay`), `overlay_convert.rs`
  (`draw_convert_overlay`), `overlay_lang_picker.rs` (`draw_lang_picker_overlay`, `lang_label`),
  `overlay_schema_enum.rs` (`draw_schema_enum_overlay`, `schema_enum_scroll_offset`,
  `schema_enum_page_step`); `ui.rs` keeps `draw`, `draw_title`, `draw_column_header`, `draw_tree`,
  `draw_status`, `draw_prompt_overlay`, and the shared cell/span helpers (`cell_preview`,
  `type_col_cell`, `value_col_width`, `edit_value_cell`, `value_cell`, `edit_field_spans`,
  `edit_overflow_hint`, `highlight_spans`, `centered_rect`, `paste_line_row`) — non-test `ui.rs`
  is now 648 lines (was 939).
- **Description**: Pure code motion, `pub(crate)` visibility for cross-module reuse. `centered_rect`
  stayed in `ui.rs` (shared by 5 of the 7 overlays) rather than a new `common.rs` — one more file
  for one four-line function wasn't worth it. `ui.rs` re-exports the handful of functions
  `mod.rs`'s event loop calls by their old `ui::X` path (`detail_full_text`, `detail_popup_rect`,
  `wrapped_line_count`, `schema_enum_page_step`, `type_filter_page_step`) so those call sites
  needed zero changes — true pure code motion, not a call-site migration.
- **Dependencies**: None (independent of the Rust-core splits in Task 15).
- **Verify**: `cargo build -p confy-tui` clean (1 unused-import warning caught and fixed:
  `type_filter_inner_height` didn't need re-exporting, only used internally by
  `overlay_type_filter.rs` itself); `cargo test -p confy-tui` unchanged at 177/177; manual TUI
  smoke test via a `hub`-launched session on a 3-node TOML fixture — Detail (`i`), Help (`?`),
  type filter (`f`), kind switch (`K`, cursor on the integer row), convert (`C`, cursor on root),
  language (`l`) all rendered correctly from their new files with live session data. Schema-enum
  picker not exercised here (no schema loaded on this plain fixture) — already covered by the
  JSON Schema feature's own prior smoke tests.

### Task 11: `SchemaSource::Url` mock-server test
- **File**: `crates/confy-tui/tests/schema_io.rs` (currently only covers `SchemaSource::Local`)
- **Description**: Add a test exercising the URL fetch path in `resolve_schema_source`. Requires
  a local mock HTTP server — check whether the workspace already has a lightweight test-server
  dependency available (`cargo tree` for `wiremock`/`httpmock`/similar in dev-dependencies); if
  none exists, use `std::net::TcpListener` + a minimal hand-rolled HTTP/1.0 response writer in a
  background thread (matches the project's low-dependency ethos — avoid adding a new crate for
  one test if a ~15-line raw listener suffices). Cover: (a) a 200 response with valid schema JSON
  resolves correctly, (b) a non-200/connection-refused resolves as a soft `load_error` (matches
  the existing local-file-missing test's soft-error convention), (c) confirm no panic on
  malformed JSON body.
- **Dependencies**: None.
- **Verify**: `cargo test -p confy-tui --test schema_io` passes, including the new URL cases.

### Task 12: JSON Schema functional_smoke coverage + resolve the waived assertion — IMPLEMENTED
- **Files**: `crates/confy-ffi/functional_smoke.mjs`
- **What shipped**:
  1. 10 new scenario-26 checks covering `SetSchema` → `schema_fetch_request`, `SchemaLoaded(Ok)` →
     `schema_status` (source label, zero violations), `BeginEdit` on an enum-constrained field
     entering `Mode::SchemaEnum` with both enum values present, `SchemaEnumMove`/`SchemaEnumCommit`
     writing the picked value, and `SchemaLoaded(Err)` resolving as a soft `load_error` with the
     document still fully editable. Proves the wasm `serde-wasm-bindgen` wire contract round-trips
     schema state — nothing did before this, despite `schema_headless.rs`'s core-layer coverage.
  2. **`grid active after toggle` root-caused and fixed** (not waived): `TypeFilter::default()`
     starts the popup cursor at `(row: 0, col: 0)`. `nav_rows()[0]` is `[Cell::Reverse]` in
     *every* format's `layout()` (`type_filter.rs`) — so toggling the default-cursor cell flips
     `reverse`, not a real facet. `TypeFilter::is_active()` (`!key_signs.is_empty() ||
     !types.is_empty()`) correctly excludes bare `reverse` — `matches()`'s own doc comment
     confirms this is deliberate: reverse alone, with nothing selected, must stay a no-op or
     toggling it would blank the whole tree before the user picked a facet. So the failure was a
     **test-script bug** (toggling the wrong cell), not a wasm/core defect, confirmed by tracing
     `type_filter_toggle`/`current_cell`/`is_active` end to end. Fixed by moving the cursor to a
     real facet cell (`TypeFilterMove(1, 0)`, landing on the Key-sign row) before toggling.
- **Dependencies**: None.
- **Verify**: `wasm-pack build --target web` + `node crates/confy-ffi/functional_smoke.mjs` — 92
  checks (was 82), 0 failures, `ALL FUNCTIONAL CHECKS PASSED` including the previously-failing
  assertion. `cargo test --workspace` / `cargo clippy --workspace` / `tsc --noEmit` (web +
  editors/vscode) all unaffected. Fixed the stale "36 checks" count in `CLAUDE.md`'s module map.

**Phase 2 testing strategy**: each task carries its own verify step above. After all of Phase 2
lands, re-run the full gate: `cargo test --workspace`, `cargo clippy --workspace`, `tsc --noEmit`,
and a manual desktop+touch+TUI smoke pass (open a sample doc, navigate, edit, open every overlay/
picker on all three surfaces).

## Phase 3 — Long-term (> 5 days)

### Task 13: Route TUI through `Session::dispatch(Intent)`
- **Files**: `crates/confy-tui/src/tui/mod.rs` (event loop, ~40 mutating call sites),
  `crates/confy-core/src/session/dispatch.rs`, `crates/confy-core/src/session/intent.rs`
- **Description**: See Architecture Decisions above — this is the structural fix behind the
  audit's top Integration finding (shift-select-reset and `ToggleExpand` branch/leaf logic
  currently hand-duplicated between `dispatch.rs`'s routing and `mod.rs`'s raw calls). Execute
  as a checklist, one Intent group at a time — checkpointed individually for groups 5-8, batched
  together for groups 1-4 (see Verify below):
  1. Navigation (`CursorDown`/`Up`/`Home`/`End`, `PageUp`/`PageDown`, `ToggleExpand`,
     `CollapseAll`/`ExpandAll`/`ExpandLevel`/`CollapseLevel`)
  2. Selection (`ToggleSelect`, `ExtendSelectUp`/`Down`)
  3. Filter + type filter (`EnterFilter`.../`EnterTypeFilter`...)
  4. Kind switch + convert (`OpenKindSwitch`.../`OpenConvert`...)
  5. Schema enum (`SchemaEnumMove`, `SchemaEnumJump`, `SchemaEnumCommit` — already dispatch-routed
     for Web; confirm TUI's direct calls at `mod.rs:318-332` behave identically once switched)
  6. Inline edit (`BeginEdit`.../`EditCommit`/`EditCancel`)
  7. Mutations + undo/redo (`Nudge`, `AddNode`, `DeleteSelected`, `CopySelected`/`CutSelected`/
     `Paste`, `Remark`, `Undo`/`Redo`)
  8. Lifecycle (`Escape`, `Save`, `QuitRequested`)

  For each group: confirm every raw call has an `Intent` equivalent (add one to `intent.rs` +
  `dispatch.rs` if genuinely missing — expect this to be rare given current coverage), replace
  the TUI event loop's raw call with `app.session.dispatch(Intent::X)`, discard/ignore the
  returned `SessionSnapshot` where the TUI still reads state directly off `app.session` afterward
  (or start threading the snapshot through if it turns out cleaner — a call during execution, not
  a pre-decided architecture point). TUI-only App-level state (`lang_picker`, and anything else
  that isn't `Session` state) stays untouched.
- **Dependencies**: None strictly, but do this **last** in the whole plan — it's the largest
  blast radius (every keyboard interaction in the TUI) and benefits from the codebase already
  being settled by Phases 1-2.
- **Verify**: groups 1-4 land as their own commits each, but share one checkpoint —
  `cargo build -p confy-tui`, `cargo test --workspace`, one manual TUI smoke pass exercising all
  four groups' keys together — before starting group 5. Groups 5-8 each get an individual
  checkpoint (same build/test/smoke steps, scoped to just that group's keys) before moving to the
  next. Final verify: the full TUI keybinding table in `README.md` §Usage, exercised end-to-end on
  a sample TOML/JSON/YAML file each.

### Task 14: Schema-aware dirty-check for revalidation + capped undo history — IMPLEMENTED
- **Files**: `crates/confy-core/src/session/session.rs` (`on_mutation_success` + its ~15 call
  sites, `apply_schema_text`), `crates/confy-core/src/schema/dirty_check.rs` (new),
  `crates/confy-core/src/schema/hints_edit.rs` (`deref` made `pub(crate)`),
  `crates/confy-core/src/schema/types.rs` (`SchemaState`), `crates/confy-core/src/session/state.rs`
  (`History`)
- **What shipped** (matches the corrected design from Architecture Decisions above):
  1. `SchemaState.fully_analyzable: bool`, computed once in `apply_schema_text` via
     `dirty_check::is_fully_analyzable`.
  2. `dirty_check::path_is_constrained` — the per-mutation O(schema-depth) walk.
  3. `on_mutation_success(&mut self, touched: Option<&Path>)` — signature change threaded through
     all ~15 call sites, but only `Path`ed at ONE: `apply_replace`'s `Some(&path)`, since it's the
     single highest-frequency call site (every value edit, rename, nudge, and schema-enum-commit
     routes through it — the "one keystroke's inline-edit commit" the audit's Critical finding
     named specifically). The other ~14 (kind-switch, comment edit/rename, structural insert,
     paste, delete-selected, remark) pass `None` — identical always-revalidate behavior to before
     this task, zero regression risk on the harder multi-path/ownership-tangled call sites. This
     differs from the literal "thread the target Path through to on_mutation_success" text above,
     which implied every call site would supply one — most of those paths are multi-node
     operations (delete-selected, paste) with no single target `Path` to thread in the first
     place, so `None`'s conservative fallback is the correct answer there, not a gap.
  4. `History::push` capped via `VecDeque` + `pop_front`, exactly as designed.
- **Dependencies**: None (see Task 9's corrected note — the two changes don't share a code path).
- **Verify**: `cargo test -p confy-core` — `schema_headless.rs` 48→51, `session_headless.rs`
  80→82, rest unchanged (472 lib tests). New tests verify behaviorally via a sentinel `Violation`
  planted before each mutation (survives = skipped; overwritten = revalidated) rather than Vec
  pointer identity, which can coincidentally match on allocator reuse and would have been a flaky
  test: (a) unconstrained path on a `fully_analyzable` schema — sentinel survives; (b) `allOf`
  schema (not `fully_analyzable`) — sentinel always overwritten regardless of path; (c) constrained
  path — sentinel overwritten, real new `Violation` present. History: (d) exactly 200 survive
  after 250 pushes, all 200 undoable, the 201st `undo()` has nothing (genuinely evicted, not just
  hidden past the cap); undo/redo correct at the boundary. `cargo clippy --workspace` 0 warnings.
  `functional_smoke.mjs` 92/92 unchanged (`fully_analyzable` is core-internal, not part of the
  wasm wire contract).

### Task 15: Split oversized files into per-concern modules — IMPLEMENTED
- **What shipped** (all 3 files, corrected scope — see below):
  - `cst_edit.rs` (7063 ln, 97 top-level items) → 9 modules: `mod.rs` (dispatcher: `apply`,
    `validate_semantics`, `serialize_fragment*`, `joinable_entry`), `escape.rs`, `convert.rs`
    (renamed from `scalar_convert` — covers container conversions too, `convert_kind` dispatches
    to all three), `dotted_table.rs`, `aot_group.rs`, `move_paste.rs`, plus 3 the plan's own
    5-module sketch didn't cover (verified by mapping every item programmatically — the sketch
    accounted for only 53 of 97 items): `replace_delete.rs`, `rename.rs`, `tree_nav.rs`.
  - `yaml/edit.rs` (3957 ln, 80 items) → 6 modules: `mod.rs`, `resolve.rs`, `block.rs`, `flow.rs`,
    `convert.rs` — matching the plan's own sketch closely — plus one addition: `mutations.rs`
    (rename/remark/comment/move/trailing-comment, uncovered by the original 4-module sketch).
  - `session.rs` (3713 ln, one `impl Session` of 146 methods + 16 free functions — structurally
    different from the other two, not free-standing functions) → 5 new siblings exactly as
    planned: `inline_edit.rs`, `clipboard.rs`, `undo_redo.rs`, `schema_hint.rs`, `status_fmt.rs`,
    each a fragment `impl Session { ... }` (Rust allows inherent impls to split across
    files/modules) or free-function module; `session.rs` keeps the struct + a 105-method core
    `impl Session` + the test module.
- **Method**: for each file, extracted the full source verbatim and computed exact item
  boundaries (doc-comment + signature + brace-matched body) programmatically rather than by
  hand — files this size make manual line-counting unreliable. Verified 1:1 coverage (every item
  assigned to exactly one module, none missing/duplicated) before generating anything. Every
  cross-module reference got `pub(crate)` (methods: only where actually called from a different
  file, computed by scanning every method body + test module for cross-bucket references, not
  guessed; free functions/types: same, plus an explicit `use` since free-function calls — unlike
  `self.method()` — need the name in scope). Let the compiler find the real gaps my
  regex-based dependency scan missed (a handful each time — a value used as a bare type instead
  of a call, a function passed as a fn-pointer instead of being called directly, a test-only
  reference my method-body-only scan didn't check) rather than hand-verifying every reference.
  `cargo fix --lib --tests -p confy-core` (not just `--lib`) cleaned the resulting unused-import
  churn — `--tests` matters: a `--lib`-only fix pass can strip an import a `#[cfg(test)]` block
  still relies on transitively, which `--lib`'s non-test compile can't see.
- **Corrected from the plan's original 3-file/13-module sketch**: 4 modules were missing outright
  (`replace_delete.rs`, `rename.rs`, `tree_nav.rs` for `cst_edit.rs`; `mutations.rs` for
  `yaml/edit.rs`) — the plan's own module lists only accounted for 53/97 and 66/80 items
  respectively; the remainder (whole-node replace/delete/rename, low-level tree navigation,
  comment/trailing-comment mutations) didn't fit any named bucket. Corrected by completing the
  item-by-item map before writing any file, not by guessing at boundaries during execution.
- **Dependencies**: `session.rs` split ran after Task 14 (already landed) per the plan's own
  sequencing note.
- **Verify**: `cargo build --workspace` clean after each file. `cargo test -p confy-core`
  472/472 unchanged after all 3 (proves zero behavior drift — no test file content was ever
  edited, only `use` imports extended). `cargo test --workspace` unchanged. `cargo clippy
  --workspace --all-targets` (lib + test targets, not just lib) 0 warnings — confirms no
  unused-import residue survived, including inside `#[cfg(test)]` blocks.
- **Line counts, before → after** (non-test lines; a few remain over the 800-line threshold —
  each is one coherent concern, not worth further fragmentation for the indirection cost):
  `cst_edit.rs` 7063 → 9 files (126–1107 ln each, 2 over 800: `move_paste.rs` 999,
  `replace_delete.rs` 1107); `yaml/edit.rs` 3957 → 6 files (64–820 ln each, 1 at the threshold:
  `block.rs` 820); `session.rs` 3713 → 6 files (53–1945 ln each — `session.rs` itself, at 1945
  total incl. its ~170-line test module, is the one file still clearly over threshold: 105
  cohesive core-API methods have no further natural seam).

### Task 16: Row-level diffing / virtualization for tree rendering
- **Files**: `web/render.ts:192-226` (`renderTree`, `treeEl.innerHTML = rows.map(...).join('')`),
  `crates/confy-tui/src/tui/ui.rs:261-407` (`draw_tree`, post-Task-10 lives wherever `draw_tree`
  landed — it stays in the main `ui.rs`, not an overlay file)
- **Description**: Two independent sub-tasks (different runtimes, no shared code):
  1. **Web**: replace unconditional `innerHTML` rebuild with keyed row diffing — build the new
     row list, compare against the previously-rendered `data-path` keys, patch only rows that
     changed (add/remove/reorder/update text), leave untouched rows' DOM nodes alone. This
     preserves focus/selection/scroll-position implicitly (a longstanding side benefit) and cuts
     per-navigation-keystroke cost from O(visible rows) DOM rebuild to O(changed rows). A
     from-scratch keyed-diff implementation (no virtual-DOM library dependency, matching the
     project's zero-framework web stack) — insert/remove/move nodes by `data-path` key
     comparison, similar in spirit to a minimal "list reconciliation" algorithm.
  2. **TUI**: `draw_tree` already receives `TableState`'s scroll `offset`, but discards it when
     building the `Vec<Row>` — rebuilds all logical rows every draw call. Change it to slice
     `app.session.visible_rows()` (or the new O(1)-indexed accessor from Task 9) to just the
     rows within `[offset, offset + viewport_height)` before building `Vec<Row>`, matching what
     `ratatui::widgets::Table` actually renders. This is a smaller, more surgical change than the
     web side.
- **Dependencies**: Sequence after Task 9 (TUI side benefits from the same row-lookup
  infrastructure) and ideally after Task 7 (web side's shared row builder makes keyed diffing
  cleaner to implement against one row-HTML function instead of two).
- **Verify**: Manual smoke test on a large synthetic document (a few thousand rows) on both web
  and TUI — confirm scroll/navigation stays smooth and visually identical to before (no missed
  updates, no stale rows). `cargo test -p confy-tui` unchanged pass count (add a new test
  asserting `draw_tree`'s row count matches the viewport height, not the full expanded-row
  count, for a document larger than the terminal). No automated web test exists for render
  output today (per the audit's Testing note) — this task does not introduce one; visual/manual
  verification only, consistent with the existing convention.

**Phase 3 testing strategy**: this phase is architecturally the riskiest — land one task at a
time, full `cargo test --workspace` + `tsc --noEmit` + manual smoke pass after each task, not
just at the end of the phase. Task 13 in particular should be treated as its own mini-project
with per-group checkpoints (see its own verify step).

## Cross-cutting Integration Points

- Tasks 9, 14, and the `session.rs` portion of Task 15 all touch the mutation-commit path
  (`on_mutation_success` and its call sites) — sequence them in that order (9 → 14 → 15) rather
  than in parallel, to avoid three separate agents racing edits to the same ~20-line function.
- Task 13 (TUI dispatch routing) should run after Phase 2's Task 10 (overlay split) lands, since
  Task 13 touches `mod.rs`'s event loop which references overlay-drawing functions whose module
  path will have changed.
- None of these tasks touch `confy-tauri`, `tauri-plugin-confy-picker` beyond Task 3, or Android/
  iOS-specific code — no mobile re-verification needed for Phases 2-3.

## Metrics / Definition of Done

- `cargo clippy --workspace`: 0 warnings (currently 1).
- `cargo test --workspace`: unchanged or higher pass count, 0 failures, throughout.
- `tsc --noEmit` (web): 0 errors, throughout.
- Files over the 800-line threshold: 8 → 3 (`json/edit.rs` 2116, `ui.ts` 1856, `touch/app.ts` 1442
  are **not** in this plan's scope — the audit flagged them but the Prioritized Action Plan only
  commits to splitting the 3 largest core files; `ui.ts`/`touch/app.ts` structural splits are a
  plausible future follow-up but weren't listed in the audit's own action plan, so excluded here
  to match what was actually approved).
- `crates/confy-ffi/functional_smoke.mjs`: JSON Schema scenarios added; `grid active after
  toggle` no longer silently waived.
