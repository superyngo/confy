✅ **Shipped — historical reference.** See `CHANGELOG.md` for current behavior; this plan is kept for context, not as a live task list.

# Schema Warning Indicators — Type Filter Facet + Collapsed-Branch Marker

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [x]`) syntax for tracking.

**Goal:** Two independent, additive improvements to schema-warning discoverability, agreed via
the grilling session recorded in this session's transcript:

1. A collapsed branch whose subtree contains at least one schema violation shows a lightweight
   marker (TUI: `⚠` glyph; web/touch: an amber corner-dot badge in the existing violation
   vocabulary), so the user doesn't have to blindly expand every branch to find a hidden warning.
   The marker composes with, but is visually distinct from, the existing Locked-selection `●`
   glyph and the existing per-row yellow/amber "this row itself violates" accent.
2. The `f` type-filter popup gains a third, independent facet ("Flags": has-schema-warning),
   ANDed with the existing Type/Sign facets and participating in the existing `Reverse` cell.

**Order (per grilling session):** Phase 1 (collapsed-branch marker, TUI only) → Phase 2 (type
filter facet, TUI only, core changes auto-propagate the popup to web/touch) → Phase 3 (port
Phase 1's row-level marker to web/touch — Phase 2 needs no web/touch-specific work since those
hosts render the type-filter popup generically from core's `layout()`/`Cell` data).

**Architecture:**
- Phase 1 reuses the *ancestor-marking walk* pattern `recompute_filter` already uses
  (`crates/confy-core/src/session/session.rs:806-811`) — when a violation's path is known, walk
  its ancestor chain and record membership in a `HashSet<Path>`. This set is rebuilt at the same
  single convergence point that already exists for revalidation:
  `Session::revalidate_schema()` (`session.rs:1525-1540`), called from `apply_schema_text` and
  from the dirty-check-gated path in mutation handlers — no new call sites, no extra revalidation
  passes.
- Phase 2 threads violation-presence into the existing `TypeFilter::matches`/`base_match`
  predicate chain (`crates/confy-core/src/session/type_filter.rs:355-401`) as a new orthogonal
  boolean facet, not a new `TypeToken` — schema-warning status is runtime validation state, not a
  lexical/structural type, so it doesn't belong in `classify()`.
- Both phases are purely additive: no existing `Intent`, wire field, or CSS rule is removed or
  repurposed. `ViewRow`/`SessionSnapshot` gain new optional/boolean fields; existing consumers
  that don't know about them are unaffected (serde defaults / additive fields).

**Tech Stack:** Rust + ratatui 0.28 (TUI), TypeScript + esbuild + plain CSS custom properties
(web/touch), `jsonschema` crate (schema validation, unchanged).

**Spec inputs:** This plan's own grilling-session transcript (design tree, all forks resolved);
`ROW_STATE_MODEL.md` §3 (existing visual-language table, to be extended, not touched); prior art
`docs/superpowers/plans/2026-08-18-row-state-visual-language-phase1.md` (structural template for
this doc).

## Global Constraints

- Collapsed-branch marker color: reuse `Color::Yellow` (TUI, `ui.rs:394`) / the existing amber
  token web already uses for `.schema-violation` — no new color introduced.
- Collapsed-branch marker glyph (TUI): `⚠`, placed in the NAME cell *after* the existing `sel_marker`
  (`●`) and *before* the tree indent/caret, so a row that is both locked-selected and
  warning-flagged shows both (`● ⚠ server`). Do not touch the existing `●` glyph or its
  positioning logic (`ui.rs:332-337`).
- Marker visibility rule: `is_branch && !expanded(path) && contains_descendant_warning(path)`.
  This is evaluated fresh per row at render time from a precomputed `HashSet<Path>` — no special
  "clear on expand" or "transfer to next collapsed ancestor" logic is written; both behaviors are
  emergent from re-evaluating the same predicate at every depth on every render.
- The marker ignores the active text/type filter (`filtered_paths`) — it reflects real document
  structure only, per the grilling session's explicit decision.
- Type-filter Phase 2 facet participates in the existing `Reverse` cell — no separate reverse
  toggle is added.
- No existing `Intent` signature changes for user-facing behavior other than what's listed above.
  `ViewRow`/`TypeFilter`/`SchemaState` gain fields; nothing is removed.

---

### Task 1: Core — compute the `has_descendant_warning` ancestor set

**Files:**
- Modify: `crates/confy-core/src/schema/types.rs` (`SchemaState`, add
  `pub warning_ancestors: HashSet<Path>` alongside `violations`).
- Modify: `crates/confy-core/src/session/session.rs:1525-1540` (`revalidate_schema`) — after
  computing `state.violations`, rebuild `state.warning_ancestors` from it.
- Test: append to `crates/confy-core/tests/schema_headless.rs`, alongside the existing
  `dispatch_schema_loaded_populates_snapshot_status_and_row_warnings` test (`:674`).

**Interfaces:**
- Consumes: `Vec<Violation>` (already computed), each `Violation.path: Path`.
- Produces: `SchemaState::warning_ancestors: HashSet<Path>` — for every violation, every strict
  ancestor prefix of `violation.path` (not the violating path itself; that node's own accent is
  already handled by the pre-existing per-row `violations` lookup in `to_view_row`).

- [x] **Step 1: Write the failing test**

In `crates/confy-core/tests/schema_headless.rs`, add:

```rust
#[test]
fn revalidate_schema_marks_ancestors_of_violating_paths() {
    let mut s = session_from(
        "[server]\nport = \"nope\"\n",
        DocFormat::Toml,
    );
    let schema_text = json!({
        "type": "object",
        "properties": {
            "server": {
                "type": "object",
                "properties": { "port": { "type": "integer" } }
            }
        }
    })
    .to_string();
    s.apply_schema_text(SchemaSource::Local("/tmp/s.json".into()), Ok(schema_text));
    let state = s.schema.as_ref().unwrap();
    assert!(!state.violations.is_empty(), "port must violate (string vs integer)");
    let server_path = vec![Seg::Key("server".into())];
    assert!(
        state.warning_ancestors.contains(&server_path),
        "server (ancestor of violating port) must be marked"
    );
    // Root path (empty Vec) is also a strict ancestor of everything — include it too,
    // matching `recompute_filter`'s existing ancestor-walk convention.
    assert!(state.warning_ancestors.contains(&vec![]));
}
```

- [x] **Step 2: Run the test to verify it fails**

Run: `cargo test -p confy-core --test schema_headless revalidate_schema_marks_ancestors_of_violating_paths`

Expected: FAIL (compile error — `warning_ancestors` doesn't exist yet).

- [x] **Step 3: Implement**

In `crates/confy-core/src/schema/types.rs`, add the field to `SchemaState` (after `violations`):

```rust
    pub violations: Vec<Violation>,
    /// Every strict ancestor path of every current violation, including the
    /// root (`vec![]`) — lets a collapsed branch row show a "warning inside"
    /// marker without walking the whole subtree per render. Rebuilt in
    /// lockstep with `violations` by `Session::revalidate_schema`.
    pub warning_ancestors: std::collections::HashSet<Path>,
```

Initialize it wherever `SchemaState` is constructed in `apply_schema_text` (`session.rs:~1471-1518`
— re-read before editing, this plan doesn't reproduce that constructor) with `HashSet::new()`.

In `crates/confy-core/src/session/session.rs`, extend `revalidate_schema` (`:1525-1540`):

```rust
    pub fn revalidate_schema(&mut self) {
        let Some(state) = self.schema.as_mut() else {
            return;
        };
        let Some(compiled) = state.compiled.as_ref() else {
            return;
        };
        let Some(doc) = self.doc.as_ref() else { return };
        let Ok((value, _warnings)) = doc.to_value() else {
            return;
        };
        let (projection, map) = crate::schema::value_bridge::bridge(&self.tree.root, &value);
        state.violations = crate::schema::validate::validate(&projection, compiled, &map);
        state.warning_ancestors = state
            .violations
            .iter()
            .flat_map(|v| (0..v.path.len()).map(|i| v.path[..i].to_vec()))
            .collect();
    }
```

- [x] **Step 4: Run the test to verify it passes**

Run: `cargo test -p confy-core --test schema_headless revalidate_schema_marks_ancestors_of_violating_paths`

Expected: PASS.

- [x] **Step 5: Run the full core test suite**

Run: `cargo test -p confy-core`

Expected: PASS (additive field, no existing assertion touches it).

- [x] **Step 6: Commit**

```bash
git add crates/confy-core/src/schema/types.rs crates/confy-core/src/session/session.rs crates/confy-core/tests/schema_headless.rs
git commit -m "feat(core): track schema-warning ancestor paths on revalidate"
```

---

### Task 2: Core — surface `has_descendant_warning` on `ViewRow`

**Files:**
- Modify: `crates/confy-core/src/session/view.rs` (`ViewRow` struct, `:40-70`) — add
  `pub has_descendant_warning: bool`.
- Modify: `crates/confy-core/src/session/session.rs` (`to_view_row`, `:142-172`) — populate it.
- Test: append to `crates/confy-core/tests/schema_headless.rs`.

**Interfaces:**
- Consumes: `self.schema.as_ref().map(|s| &s.warning_ancestors)` (Task 1's new field).
- Produces: `ViewRow::has_descendant_warning: bool` — `true` only for branch rows whose path is
  in `warning_ancestors`. Deliberately computed for **every** branch row regardless of its
  current expand state — the renderer (Task 3) is what gates display on "currently collapsed";
  keeping the field expand-state-agnostic here keeps `to_view_row` a pure function of
  `(node, depth)` plus session state, matching its existing contract.

- [x] **Step 1: Write the failing test**

Extend the Task 1 test (or add a sibling) in `schema_headless.rs`:

```rust
#[test]
fn collapsed_ancestor_row_reports_has_descendant_warning() {
    let mut s = session_from("[server]\nport = \"nope\"\n", DocFormat::Toml);
    // ... same schema as Task 1 ...
    s.apply_schema_text(SchemaSource::Local("/tmp/s.json".into()), Ok(schema_text));
    s.toggle_expand(&vec![Seg::Key("server".into())]); // collapse it (starts expanded)
    let rows = s.visible_rows();
    let server_row = rows.iter().find(|r| r.key == "server").unwrap();
    assert!(server_row.is_branch);
    assert!(server_row.has_descendant_warning);
}
```

(Verify the exact collapse-toggle method name — likely `toggle_expand`/`collapsed.insert`; grep
`session.rs` for the existing expand/collapse mutator before writing this step for real, since
this plan does not reproduce it verbatim.)

- [x] **Step 2: Run to verify it fails** — `cargo test -p confy-core --test schema_headless collapsed_ancestor_row_reports_has_descendant_warning` → FAIL (field doesn't exist).

- [x] **Step 3: Implement**

`view.rs`, inside `ViewRow` (after the existing `violations` field, `:69`):

```rust
    pub violations: Option<Vec<String>>,
    /// `true` when this row is a branch and some node in its subtree (at any
    /// depth) currently has a schema violation — independent of this row's
    /// own expand state; the renderer decides whether to draw a marker based
    /// on whether the row is *currently* collapsed.
    pub has_descendant_warning: bool,
```

`session.rs`, inside `to_view_row` (`:147-171`), add the field to the `ViewRow { ... }` literal:

```rust
            has_descendant_warning: node.is_branch()
                && self
                    .schema
                    .as_ref()
                    .is_some_and(|s| s.warning_ancestors.contains(&node.path)),
```

- [x] **Step 4: Run to verify it passes**, **Step 5: full core suite**, **Step 6: commit**
      (same pattern as Task 1).

```bash
git add crates/confy-core/src/session/view.rs crates/confy-core/src/session/session.rs crates/confy-core/tests/schema_headless.rs
git commit -m "feat(core): expose has_descendant_warning on ViewRow"
```

---

### Task 3: TUI — render the `⚠` marker on collapsed branches

**Files:**
- Modify: `crates/confy-tui/src/tui/ui.rs:318-397` (row-render loop — `sel_marker`/`prefix`
  construction and the `base` style block).
- Test: append to `ui.rs`'s existing `mod tests` block, following the buffer-inspection pattern
  already used by `cursor_selection_and_clip_source_colors_are_distinct_and_composable`
  (`docs/superpowers/plans/2026-08-18-row-state-visual-language-phase1.md:59-91` for the idiom;
  the real target file is `ui.rs`, not that historical plan doc).

**Interfaces:**
- Consumes: `row.has_descendant_warning: bool` (Task 2), `app.is_expanded(&row.path)` (existing).
- Produces: no new public API — presentation only.

- [x] **Step 1: Write the failing test**

```rust
#[test]
fn collapsed_branch_with_descendant_warning_shows_marker_glyph() {
    let mut app = App::new(crate::model::any_doc::AnyDocument::Toml(
        crate::model::cst_doc::CstDocument::from_str("[server]\nport = \"nope\"\n").unwrap(),
    ));
    app.session.apply_schema_text(
        confy_core::schema::SchemaSource::Local("/tmp/s.json".into()),
        Ok(r#"{"type":"object","properties":{"server":{"type":"object","properties":{"port":{"type":"integer"}}}}}"#.to_string()),
    );
    app.rebuild_rows();
    // Collapse `server` — find its row and toggle.
    let server_idx = app.rows.iter().position(|r| r.key == "server").unwrap();
    app.session.cursor = app.rows[server_idx].path.clone();
    app.toggle_expand(); // however this repo's existing collapse action is invoked from App
    app.rebuild_rows();
    let mut terminal = Terminal::new(TestBackend::new(40, 8)).unwrap();
    terminal.draw(|fr| draw(fr, &app)).unwrap();
    let buf = terminal.backend().buffer().clone();
    assert!(
        (0..40).any(|x| (0..8).any(|y| buf[(x, y)].symbol() == "⚠")),
        "collapsed branch with a hidden violation must show the ⚠ marker"
    );
}
```

(Re-verify `App`'s real expand/collapse method name and whether `rebuild_rows` is required after
`apply_schema_text` before writing this for real — this plan's job is to name the target
behavior, not guess unread method names.)

- [x] **Step 2: Run to verify it fails.**

- [x] **Step 3: Implement**

In `ui.rs`, extend the `sel_marker`/`prefix` construction (`:332-337`):

```rust
            let sel_marker = if app.session.selection.contains(&row.path) {
                "●"
            } else {
                " "
            };
            let warn_marker = if row.is_branch
                && row.has_descendant_warning
                && !app.is_expanded(&row.path)
            {
                "⚠"
            } else {
                " "
            };
            let prefix = format!("{sel_marker}{warn_marker}{indent}{marker}");
```

No change needed to the `base` style block (`:388-397`) — the marker is a glyph within the
existing `name` `Cell`, not a background fill, so it composes with every existing row style
(cursor blue, clip-source green/purple, or the plain/violation-yellow base) without a new style
arm.

- [x] **Step 4: Run to verify it passes.**

- [x] **Step 5: Run the full TUI suite** — `cargo test -p confy-tui` — confirm no existing test
      asserted a fixed 2-character prefix width that this 3rd character now breaks (search for
      any test slicing `name`/`prefix` by fixed column offsets before assuming none exist).

- [x] **Step 6: Manual verification on the real binary** (per this session's Bug-Fix/UI-change
      protocol — a green unit test is not suf1ficient on its own for a visual change):

Run: `printf '[server]\nport = "nope"\n' > /tmp/w.toml && echo '{"type":"object","properties":{"server":{"type":"object","properties":{"port":{"type":"integer"}}}}}' > /tmp/w.json && cargo run -p confy-tui -- --schema /tmp/w.json /tmp/w.toml`

Expected: `server` row (collapsed by default, since only a file's immediate children start
expanded — confirm actual default) shows `⚠` before its caret; pressing `Space` to expand it
reveals `port` in yellow with its own violation styling, and the `⚠` on `server`'s own row
disappears (server itself isn't collapsed anymore — nothing left to summarize). Also verify: with
the `port` row locked-selected (`s`) and the branch containing it collapsed simultaneously
(select an ancestor, not `port` itself, since `port` is a leaf) — not directly testable in this
exact document; verify the `●`+`⚠` co-occurrence claim with a document that has a locked-selected
*branch* that also collapses over a warning, e.g. select `server` itself while it's collapsed.

- [x] **Step 7: Commit**

```bash
git add crates/confy-tui/src/tui/ui.rs
git commit -m "feat(tui): mark collapsed branches containing a hidden schema warning"
```

---

### Task 4: Core — add the "has schema warning" facet to the type filter

**Files:**
- Modify: `crates/confy-core/src/session/type_filter.rs` — `TypeFilter` struct (`:329-336`),
  `base_match`/`matches`/`is_reverse_excluded` (`:355-401`), `Cell` enum (`:132-140`), `layout()`
  (`:205-310`), `nav_rows()` (`:312-320`), `toggle`/`cell_state` (`:435-480`).
- Modify: `crates/confy-core/src/session/session.rs` — `recompute_filter`'s `walk` (`:770-836`),
  threading violation-presence through to `type_filter.matches(...)` (`:806`).
- Test: extend `type_filter.rs`'s existing `mod tests` block (`:491-758`).

**Interfaces:**
- Consumes: per-node violation presence — the walk in `recompute_filter` already has
  `self.schema` in scope, so it can look up `self.schema.as_ref().is_some_and(|s|
  s.violations.iter().any(|v| v.path == n.path))` per node (or, cheaper: build one
  `HashSet<Path>` of violating paths once before the walk starts, mirroring how `warning_ancestors`
  is precomputed in Task 1 — prefer this to avoid an O(violations) scan per node).
- Produces: `TypeFilter::warning_only: bool` (new field, same shape as `reverse`); `Cell::Warning`
  (new variant); a new "Flags" `LayoutRow` group in `layout()`.

- [x] **Step 1: Write the failing tests**

```rust
#[test]
fn warning_facet_matches_only_violating_nodes() {
    let mut f = TypeFilter::default();
    f.toggle(Cell::Warning);
    assert!(f.matches_with_warning(KeySign::Bare, &NodeKind::Scalar(ScalarType::Integer), Format::Plain, DocFormat::Toml, false, true));
    assert!(!f.matches_with_warning(KeySign::Bare, &NodeKind::Scalar(ScalarType::Integer), Format::Plain, DocFormat::Toml, false, false));
}

#[test]
fn warning_facet_composes_with_reverse() {
    let mut f = TypeFilter::default();
    f.toggle(Cell::Warning);
    f.toggle(Cell::Reverse);
    // reverse + has-warning = "only nodes WITHOUT a warning"
    assert!(!f.matches_with_warning(KeySign::Bare, &NodeKind::Scalar(ScalarType::Integer), Format::Plain, DocFormat::Toml, false, true));
    assert!(f.matches_with_warning(KeySign::Bare, &NodeKind::Scalar(ScalarType::Integer), Format::Plain, DocFormat::Toml, false, false));
}
```

(Exact fn name `matches_with_warning` vs. extending `matches`'s existing signature in place is an
implementation choice for Task 4's author — extending `matches` in place is preferred so
`recompute_filter`'s single call site (`session.rs:806`) doesn't need two branches; the test names
above assume that choice. Confirm no other caller of `TypeFilter::matches` exists that can't
supply a violation flag before committing to signature-extension over a new method — grep
`\.matches(` scoped to `type_filter` callers first.)

- [x] **Step 2: Run to verify failure** (compile error — `Cell::Warning` doesn't exist).

- [x] **Step 3: Implement**

`Cell` enum (`:132-140`): add a variant, e.g. `Cell::Warning` (sibling to `Cell::Reverse`, not
nested under `Sign`/`Token`/`All`).

`Cell::label()` (`:143-155`): add `Cell::Warning => "(!) has warning"`.

`TypeFilter` struct (`:329-336`): add `pub warning_only: bool` alongside `pub reverse: bool`.

`clear()` (`:343-347`): reset `self.warning_only = false;` too.

`is_active()` (`:339-341`): **do not** add `warning_only` here — `is_active` currently means "a
Type/Sign facet is selected" and gates whether `Reverse` is a no-op (`matches`, `:396`) and
whether the whole `TypeFilter` counts as active for `recompute_filter`'s early-return (`:771`).
Confirm during implementation whether `warning_only` alone (with no Type/Sign selected) should
independently activate filtering — the grilling session implies yes (it's an independent facet a
user can use alone), so `is_active()` becomes `!self.key_signs.is_empty() || !self.types.is_empty()
|| self.warning_only`, and `recompute_filter`'s early-return condition
(`self.filter.is_empty() && !self.type_filter.is_active()`, `session.rs:771`) picks this up for
free since it already calls `is_active()`.

`base_match`/`matches`/`is_reverse_excluded` (`:355-401`): thread a new `has_warning: bool`
parameter through all three; `base_match` becomes `sign_ok && type_ok && warning_ok` where
`warning_ok = !self.warning_only || has_warning`.

`toggle()` (`:435-460`): add `Cell::Warning => self.warning_only = !self.warning_only,`.

`cell_state()` (`:473-480`): add `Cell::Warning => bool_state(self.warning_only),`.

`layout()` (`:205-310`) / `nav_rows()` (`:312-320`): re-read the full function bodies before
editing (elided above) — add one new `LayoutRow` section (header text "Flags") containing a
single `Cell::Warning` cell, and ensure `nav_rows()` includes it so keyboard navigation reaches
it.

`session.rs`'s `recompute_filter` `walk` (`:770-836`): before the walk starts, precompute
`let violating: HashSet<&Path> = self.schema.as_ref().map(|s| s.violations.iter().map(|v|
&v.path).collect()).unwrap_or_default();` (borrow-checker permitting — may need `.clone()`d
`Path`s instead of refs depending on `walk`'s existing lifetime shape; re-read `walk`'s signature
before committing to borrowed vs owned). Then at `:806`, change:

```rust
            let type_ok = type_filter.matches(n.key_sign, &n.kind, n.format, doc, n.read_only);
```
to also pass `violating.contains(&n.path)` (or equivalent) into `matches`.

- [x] **Step 4: Run tests to verify pass.**

- [x] **Step 5: Full core suite** — `cargo test -p confy-core`.

- [x] **Step 6: Commit**

```bash
git add crates/confy-core/src/session/type_filter.rs crates/confy-core/src/session/session.rs
git commit -m "feat(core): add has-schema-warning facet to the type filter (AND + reverse-compatible)"
```

---

### Task 5: TUI — verify the popup renders the new facet with no TUI-specific code

**Files:** none expected to change — `overlay_type_filter.rs` and `type_filter.rs` (TUI wrapper,
`pub use ... layout ...`) are generic over whatever `layout()` returns.

- [x] **Step 1: Manual verification on the real binary**

Run the same `/tmp/w.toml` + `/tmp/w.json` fixture from Task 3, press `f`, confirm a "Flags"
section with a "(!) has warning" cell appears, toggling it filters the tree to only
schema-violating nodes (plus their ancestors, via the existing ancestor-inclusion behavior at
`session.rs:807-811` — this comes for free since that code path is shared, not per-facet).

- [x] **Step 2: If the popup layout looks cramped or the new section's header styling doesn't
      match Type/Sign's existing header treatment, fix in `overlay_type_filter.rs` — otherwise no
      commit needed for this task (verification-only).**

---

### Task 6: Web/touch — port the collapsed-branch marker

**Files:**
- Modify: `web/types.ts` (`ViewRow` interface — add `has_descendant_warning: boolean`).
- Modify: `web/render.ts` (`rowHTML`/`renderRow`, `:93-105` area) — add a corner-dot class when
  `r.is_branch && r.has_descendant_warning && !expanded(r.path)` (need the caller's existing
  expanded-state lookup — reuse whatever `renderRow`'s caller already threads through for the
  caret `▾`/`▸` glyph, do not add a second expand-state source of truth).
- Modify: `web/style.css` — new `.row.warn-branch::after` (or similar) corner-dot rule, amber,
  positioned distinctly from the existing `.schema-violation` per-row treatment so "this row
  itself violates" and "this collapsed row hides a violation" read as related-but-different.
- Modify: `web/touch/render.ts`, `web/touch/style.css` — mirror the desktop change (touch already
  mirrors desktop's clip-source/violation classes per Phase-1 precedent in the row-state plan).
- Test: extend `web/render.spec.mjs` and `web/touch-render.spec.mjs` with a snapshot/DOM-class
  assertion, following those files' existing conventions (re-read them before writing new specs —
  not reproduced here).

- [x] **Step 1–5:** standard write-test → fail → implement → pass → full web suite
      (`cd web && node run-tests.mjs` or whatever this repo's existing test command is — confirm
      via `package.json` before running) cycle, mirroring Tasks 1–3's structure.
- [x] **Step 6: Manual verification** — build and serve (`web/README.md`'s existing dev command),
      open the `/tmp/w.toml` fixture via file picker, confirm the corner dot on collapsed
      `server`, confirm it disappears on expand.
- [x] **Step 7: Commit** —
      `git commit -m "feat(web,touch): mark collapsed branches containing a hidden schema warning"`

---

## Verification Checklist (end of plan)

- [x] `cargo test -p confy-core` — PASS.
- [x] `cargo test -p confy-tui` — PASS.
- [x] `cd web && node run-tests.mjs` (confirm actual command) — PASS.
- [x] Manual: `/tmp/w.toml` + `/tmp/w.json` fixture, TUI — collapsed `server` shows `⚠`; expanding
      it reveals `port`'s own yellow violation styling; `⚠` clears on `server`'s own row.
- [x] Manual: same fixture, `f` popup — new Flags/"(!) has warning" facet filters correctly, both
      alone and combined with Reverse and with an existing Type facet.
- [x] Manual: web desktop — same two checks via browser.
- [x] Append `CHANGELOG.md` "Unreleased Update" entry (per this session's CLAUDE.md protocol)
      summarizing both features once all tasks are committed.
