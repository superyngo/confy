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

### Task 9: O(1) cursor→row lookup in `Session`
- **File**: `crates/confy-core/src/session/session.rs:132` (`visible_rows`) + ~14 call sites
  (1150, 1168, 1183, 1196, 1207, 1543, 1621, 2227, 2307, 2412, 2462, 2913, 3107, 3233, 3240 —
  re-verify each against current line numbers before editing, since Phase 1 edits shift lines)
- **Description**: Add a `row_index: Option<HashMap<Path, usize>>` (or similar) built lazily from
  `self.tree`/`self.visible_nodes()`, invalidated at every point `self.tree` is reassigned
  (`on_mutation_success`, `apply_schema_text`, undo/redo — the same set of call sites Task 14
  touches, so sequence these together or at minimum re-grep after Task 14 lands). Add a
  `fn cursor_row_index(&mut self) -> Option<usize>` (or non-mutating variant returning a fresh
  lookup if no cache exists yet) that the ~14 sites call instead of
  `self.visible_rows().iter().position(...)` / equivalent full-materialization pattern.
- **Dependencies**: Sequence after Task 14 (both touch the same tree-invalidation points) to
  avoid two separate cache-invalidation mechanisms landing in the same commit window.
- **Verify**: `cargo test -p confy-core` 472/472 unchanged (behavior-preserving — this is a perf
  optimization, not a semantic change). Add one new test asserting the O(1) path returns the
  same row the O(n) `.find()` would for a cursor mid-document, and that it's correctly invalidated
  after a mutation that changes row count (e.g. `Delete` above the cursor shifts its index).

### Task 10: Split TUI overlay renderers
- **File**: `crates/confy-tui/src/tui/ui.rs` (939 non-test lines) → `crates/confy-tui/src/tui/overlays/`
  with one file per overlay: `detail.rs` (`draw_detail_overlay`, `detail_popup_rect`,
  `detail_full_text`, `wrapped_line_count`), `help.rs` (`draw_help_overlay`), `type_filter.rs`
  (`draw_type_filter_overlay`, `type_filter_inner_height`, `type_filter_page_step`),
  `kind_switch.rs` (`draw_kind_switch_overlay`), `convert.rs` (`draw_convert_overlay`),
  `lang_picker.rs` (`draw_lang_picker_overlay`, `lang_label`), `schema_enum.rs`
  (`draw_schema_enum_overlay`, `schema_enum_scroll_offset`, `schema_enum_page_step`); `ui.rs`
  keeps `draw`, `draw_title`, `draw_column_header`, `draw_tree`, `draw_status`,
  `draw_prompt_overlay`, and the shared cell/span helpers (`cell_preview`, `type_col_cell`,
  `value_col_width`, `edit_value_cell`, `value_cell`, `edit_field_spans`, `edit_overflow_hint`,
  `highlight_spans`, `centered_rect`, `paste_line_row`).
- **Description**: Matches the crate's existing granular module convention (`keys.rs`, `search.rs`,
  `selection.rs`, `type_filter.rs` already sibling files). Pure code motion — `pub(crate)` /
  `pub(super)` visibility as needed for cross-module helper reuse (`centered_rect` is shared by
  several overlays — put it in a small `overlays/common.rs` or keep it `pub(crate)` in `ui.rs`).
- **Dependencies**: None (independent of the Rust-core splits in Task 15).
- **Verify**: `cargo build -p confy-tui` clean; `cargo test -p confy-tui` unchanged pass count;
  manual TUI smoke test (launch on a sample file, open each overlay — Detail `i`, Help `?`, type
  filter `f`, kind switch `K`, convert `C`, language `l`, and a schema-enum picker if a schema is
  loaded — confirm each still renders and responds to keys).

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

### Task 12: JSON Schema functional_smoke coverage + resolve the waived assertion
- **Files**: `crates/confy-ffi/functional_smoke.mjs`
- **Description**: Two sub-parts:
  1. Add Intent→SessionSnapshot round-trip scenarios for `SetSchema`, `SchemaLoaded`,
     `SchemaEnumMove`/`SchemaEnumCommit` — mirroring the script's existing numbered-scenario
     convention (currently 25 scenarios covering the rest of the Intent surface). Confirms the
     wasm `serde-wasm-bindgen` wire contract actually round-trips schema state, which nothing
     currently proves despite `schema_headless.rs`'s 788 lines of core-layer coverage.
  2. Investigate the long-standing `grid active after toggle` failing assertion at line 140
     (documented since 2026-08-06). Reproduce it, root-cause it (likely a `TypeFilter` state
     transition the wasm layer doesn't preserve correctly, or a scenario-ordering dependency in
     the script itself), and either fix it or — if it's genuinely out of scope for this pass —
     convert the silent pass/fail waiver into an explicit `console.warn` + tracked follow-up
     reference (not a bare hidden waiver inside a script that gates merges by convention).
- **Dependencies**: None.
- **Verify**: `node crates/confy-ffi/functional_smoke.mjs` (after `wasm-pack build --target web`)
  — all scenarios pass including the new schema ones; the `grid active after toggle` assertion
  either passes or is explicitly, visibly flagged (not silently skipped).

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

### Task 14: Schema-aware dirty-check for revalidation + capped undo history
- **Files**: `crates/confy-core/src/session/session.rs:2501-2515` (`on_mutation_success`),
  `:1379-1391` (`revalidate_schema`), `:1337-1373` (`apply_schema_text`),
  `crates/confy-core/src/schema/types.rs:112-127` (`SchemaState`),
  `crates/confy-core/src/schema/hints_edit.rs` (reference only — new sibling module, not this
  file, hosts the new walk), `crates/confy-core/src/session/state.rs:211-229` (`History`)
- **Description**: Two related changes, corrected from the initial draft after fact-finding
  showed the original "reuse EditHint's precomputed constrained paths" premise was false — no
  such cache exists; `edit_hint()` re-walks the raw schema from scratch on every call, and
  `value_bridge::PointerMap` maps document structure, not schema constraints (see ADR 0003 for
  the full correction writeup):
  1. Add a `fully_analyzable: bool` field to `SchemaState`, computed once in `apply_schema_text`
     by walking the raw schema document and confirming it contains no remote `$ref`,
     `allOf`/`not`/`if-then-else`, or `oneOf`/`anyOf` beyond `hints_edit.rs`'s existing `const`
     carve-out, anywhere. This is a whole-schema-document walk, done once per
     `SetSchema`/`SchemaLoaded`, not per mutation. Add a new module
     `crates/confy-core/src/schema/dirty_check.rs` with a sibling function to
     `hints_edit::resolve_subschema` (sharing its `deref` helper) that, when `fully_analyzable` is
     true, walks from the schema root along a target `Path` and returns whether any keyword
     (`type`/`enum`/`const`/`pattern`/bounds/`required`/etc.) applies there. Deliberately a new
     module rather than added to `hints_edit.rs`: that file's `None` return means "safe to fall
     back to plain-text editing" (conservative-if-unsure-say-no-hint); a dirty-check's "unsure"
     case must mean the opposite polarity ("assume constrained, revalidate") — mixing the two in
     one file risks a future edit silently flipping the wrong one's safe direction.
  2. In `on_mutation_success`, before calling `revalidate_schema()`: if `schema.is_none()` (no
     schema loaded) or `!schema.fully_analyzable`, behavior is unchanged (always revalidate — the
     safe default). If `fully_analyzable`, thread the just-applied `Mutation`'s target `Path`
     through to `on_mutation_success` (it doesn't receive one today) and call the new dirty-check;
     skip `revalidate_schema()` and reuse the previous `violations` list untouched only when the
     check conclusively finds no constraint on that path.
  3. `History::push` (state.rs:225-228): cap `past` at a **fixed 200 entries** (not configurable —
     see ADR 0003) via a `VecDeque` ring buffer, evicting the oldest entry with `pop_front()` (not
     `Vec::remove(0)`, which is O(n) and would defeat the purpose on every commit).
- **Dependencies**: Sequence with Task 9 (both touch mutation-commit invalidation points).
- **Verify**: `cargo test -p confy-core` 472/472 unchanged. New tests: (a) a `fully_analyzable`
  schema — a mutation entirely outside any constrained path does not trigger `iter_errors`
  (assert via a call-counter or pointer-identity on the unchanged `violations` Vec); (b) a schema
  that is **not** `fully_analyzable` (e.g. contains `allOf`) always revalidates regardless of
  mutation path — proves the conservative fallback actually engages; (c) a mutation *inside* a
  constrained path on a `fully_analyzable` schema still revalidates and still surfaces a new
  `Violation` correctly; (d) `History` never exceeds the 200-entry cap after >200 edits, and
  undo/redo still work correctly at the boundary (oldest entry evicted, not the most recent).

### Task 15: Split oversized files into per-concern modules
- **Files**:
  - `crates/confy-core/src/model/cst_edit.rs` (7064 ln) → `cst_edit/mod.rs` (dispatcher: `apply`,
    `validate_semantics`) + `cst_edit/escape.rs` (`unescape_basic`, `encode_basic_string`,
    `encode_multiline_basic`, `string_inner`) + `cst_edit/scalar_convert.rs` (`convert_scalar`,
    `convert_kind` for scalars, `nudge`-adjacent helpers if any live here) + `cst_edit/dotted_table.rs`
    (`dotted_member_entries`, `replace_dotted_table`, `rename_dotted_segment`, `strip_key_prefix`,
    `dotted_ancestor_prefix_len`, `is_headerless_table`, `has_own_header`) + `cst_edit/aot_group.rs`
    (`aot_group_span`, `aot_group_insert`, `aot_entry_end`, `aot_entry_member_fragments`,
    `convert_aot_to_array`, `convert_array_to_aot`) + `cst_edit/move_paste.rs` (`move_nodes`,
    `insert`, `parse_fragment_adapted`, `wrap_keyed_as_inline_element`, `unpack_inline_table`,
    `check_partition`). Exact boundary calls are a judgment call during execution — the audit's
    own module-doc-comment groupings (lines 10-17) are the authoritative seam list; keep the
    `#[cfg(test)] mod tests` colocated with whichever module ends up easiest (or split tests
    per-module too, matching source).
  - `crates/confy-core/src/session/session.rs` (3668 ln) → keep `session.rs` as the `Session`
    struct definition + `impl Session` core (construction, doc lifecycle), extract
    `session/inline_edit.rs` (edit-buffer lifecycle: `begin_inline_edit*`, `edit_char`,
    `edit_commit`, etc.), `session/clipboard.rs` (cut/copy/paste + collision sub-state-machine),
    `session/undo_redo.rs`, `session/schema_hint.rs` (schema-hint clamping, `nudge_scalar` and
    friends), `session/status_fmt.rs` (i18n status formatting free functions at the bottom of
    the current file, lines ~3252-3500). `dispatch.rs`/`view.rs`/`selection.rs`/`insertion.rs`
    already exist as narrower siblings — new files follow that same flat `session/` layout.
  - `crates/confy-core/src/model/yaml/edit.rs` (3958 ln) → `yaml/edit/mod.rs` (dispatcher: `apply`,
    `mutation_paths`) + `yaml/edit/resolve.rs` (`resolve`, `resolve_in`, `is_opaque`, indent
    engine `reindent`) + `yaml/edit/block.rs` (block map/seq replace/delete/insert: `replace`,
    `delete`, `insert`, `find_container`, `slot_elements`, `collect_items`, `adapt_fragment`) +
    `yaml/edit/flow.rs` (all `flow_*`/`*_flow_*` functions — flow-collection edits are already a
    clearly delimited section per the file's own `// ── Flow-collection edits ──` banner) +
    `yaml/edit/convert.rs` (`convert_kind`, `convert_container`, `convert_string`, `convert_int`,
    `convert_float`, plus their string-encode/decode helpers).
- **Description**: Pure code motion, no behavior change. This is the largest and most
  mechanically risky task in the plan purely by line count — recommend one file at a time, each
  its own commit, with a full `cargo test -p confy-core` pass between files (not just at the end).
- **Dependencies**: Independent of Task 13; can run in parallel with it if desired, but NOT
  concurrently with Task 14/9 on `session.rs` (same file, sequence after those land).
- **Verify**: `cargo build --workspace` clean after each file split; `cargo test -p confy-core`
  472/472 unchanged after each; `cargo clippy --workspace` no new warnings (watch for new
  `unused import`/visibility warnings from the split — expected churn, not a regression).

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
