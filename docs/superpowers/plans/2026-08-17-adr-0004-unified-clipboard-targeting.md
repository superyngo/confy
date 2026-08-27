✅ **Shipped — historical reference.** See `CHANGELOG.md` for current behavior; this plan is kept for context, not as a live task list.

# ADR 0004: Unified Clipboard/Move Targeting Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `PasteSlot` the one target representation for copy/cut/paste/move across TUI, web keyboard, web mouse, and touch (ADR 0004 §1-§2); fix the AoT-entry atomic-move data loss (§3) and sync `CONTEXT.md` (§4).

**Architecture:** Core gains two small primitives — `Session::pointer_slot(path, rel_y) -> Option<PasteSlot>` (pixel position → target) and a `cut: bool` parameter on `move_selection_to` — plus a `paste_slot` snapshot field and a `SetPasteSlot` intent so every pointer host can read and drive the same state the TUI's arrow keys already do. Web mouse/touch stop hand-rolling their own 0.25/0.75 band-threshold + `is_branch`/`Format` eligibility checks (two independently-drifted copies today) and call the new core primitive instead. Drag-drop's own before/after sibling-index math is untouched — only its "should I offer an Into drop" eligibility check moves to core, plus a `cut` flag for the copy-modifier. The AoT fix is a single `model/cst_edit` function conditionally used instead of the existing dotted-flatten path.

**Tech Stack:** Rust (`confy-core`, `confy-ffi`/wasm-bindgen), TypeScript (`web/`, `web/touch/`), plain-Node spec tests (`esbuild` bundling, no framework), `cargo test`.

**Spec:** `docs/adr/0004-unified-clipboard-move-targeting.md`

## Global Constraints

- `SessionSnapshot`/`Intent` changes must stay wire-compatible: only additive fields (new snapshot field, new intent variant, `#[serde(default)]`-backed new struct field) — no existing variant's shape changes without a default.
- TUI is unchanged by this ADR (§2 table: "TUI | … | unchanged"). No `confy-tui` source changes in this plan except none needed — verify, don't touch.
- `web/types.ts` is the hand-written canonical TS mirror of the Rust serde shapes (its own header comment) — every Rust field/variant/enum change in this plan has a matching `types.ts` edit in the same task.
- Match existing code: JS/TS uses no test framework (`node:assert` + a `check()` tally, esbuild bundling — see `render.spec.mjs`); Rust tests follow `crates/confy-core/tests/session_headless.rs`'s `toml_session(src) -> Session` helper convention.
- Node-kind/per-format legality is `CONTEXT.md`'s job (ADR §4) — this plan updates the two spots ADR §3 changes (`Insert / move legality` notes, `Mutation mechanics` → Move row) and nothing else in that table.
- No new npm/cargo dependencies.

---

## Phase 1: Core `PasteSlot` primitive

### Task 1: `PasteSlot` becomes wire-serializable; `SessionSnapshot` gains `paste_slot`

**Files:**
- Modify: `crates/confy-core/src/session/state.rs:199-203` (`PasteSlot` derive)
- Modify: `crates/confy-core/src/session/view.rs` (imports, `SessionSnapshot` struct)
- Modify: `crates/confy-core/src/session/dispatch.rs:316-346` (`snapshot()`)
- Test: `crates/confy-core/tests/session_headless.rs`

**Interfaces:**
- Produces: `SessionSnapshot.paste_slot: Option<PasteSlot>`, surfacing `effective_paste_slot()`'s state only while the clipboard is armed (mirrors `clipboard_count`'s `Some`-only-when-nonempty convention — `None` when no clipboard is armed, `Some(slot)` otherwise).

- [ ] **Step 1: Write the failing test**

In `crates/confy-core/tests/session_headless.rs`, add a new section after the existing ones (check the file's tail for the right append point) and add `PasteSlot` to the `use confy_core::session::{...}` import list:

```rust
// ---- PasteSlot snapshot (ADR 0004 §1) ----

#[test]
fn snapshot_paste_slot_is_none_until_clipboard_armed_then_tracks_effective_slot() {
    let mut s = toml_session("a = 1\n[b]\nc = 2\n");
    assert_eq!(s.snapshot().paste_slot, None);
    s.cursor = vec![Seg::Key("a".into())];
    s.copy_selected();
    // Armed with no explicit `paste_slot` set: falls back to `After(cursor)`,
    // exactly like `effective_paste_slot()`.
    assert_eq!(
        s.snapshot().paste_slot,
        Some(PasteSlot::After(vec![Seg::Key("a".into())]))
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test session_headless snapshot_paste_slot_is_none_until_clipboard_armed_then_tracks_effective_slot -- --nocapture`
Expected: FAIL to compile — `PasteSlot` not in scope / `SessionSnapshot` has no field `paste_slot`.

- [ ] **Step 3: Implement**

In `crates/confy-core/src/session/state.rs`, extend the derive on `PasteSlot` (line 199):

```rust
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PasteSlot {
```

In `crates/confy-core/src/session/view.rs`, add the import and field:

```rust
use crate::model::document::{DocFormat, KindTarget};
use crate::session::state::PasteSlot;
```

(add `use crate::session::state::PasteSlot;` right after the existing `use crate::model::document::...` line), then add to `SessionSnapshot` (right after `clipboard_paths`):

```rust
    /// The armed clipboard's target — `effective_paste_slot()`, surfaced only
    /// while a clipboard is armed (mirrors `clipboard_count`'s convention).
    /// Every pointer host renders this instead of re-deriving it (ADR 0004 §1).
    pub paste_slot: Option<PasteSlot>,
```

In `crates/confy-core/src/session/dispatch.rs`, inside `snapshot()` (after `clipboard_paths: ...`), add:

```rust
            paste_slot: self.clipboard.as_ref().map(|_| self.effective_paste_slot()),
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test session_headless snapshot_paste_slot_is_none_until_clipboard_armed_then_tracks_effective_slot -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/confy-core/src/session/state.rs crates/confy-core/src/session/view.rs crates/confy-core/src/session/dispatch.rs crates/confy-core/tests/session_headless.rs
git commit -m "feat(core): PasteSlot serializable; SessionSnapshot gains paste_slot (ADR 0004 §1)"
```

---

### Task 2: `Session::pointer_slot` + `Session::set_paste_slot` + `Intent::SetPasteSlot`

**Files:**
- Modify: `crates/confy-core/src/session/session.rs` (new methods, after `slot_target`)
- Modify: `crates/confy-core/src/session/intent.rs` (new variant)
- Modify: `crates/confy-core/src/session/dispatch.rs:47-292` (`apply()` match arm)
- Test: `crates/confy-core/tests/session_headless.rs`

**Interfaces:**
- Consumes: `Session.tree: NodeTree`, `Session.expanded`, `Session::paste_slots` (existing, `crates/confy-core/src/session/session.rs:474`), `Session::visible_nodes` (existing, private to the crate).
- Produces: `Session::pointer_slot(&self, path: &Path, rel_y: f32) -> Option<PasteSlot>`, `Session::set_paste_slot(&mut self, slot: PasteSlot)`, `Intent::SetPasteSlot(PasteSlot)`.

- [ ] **Step 1: Write the failing tests**

Add to `crates/confy-core/tests/session_headless.rs` (extend the `use confy_core::model::node::{...}` import to include `Format`):

```rust
#[test]
fn pointer_slot_bands_into_vs_after_and_finds_the_preceding_flattened_slot() {
    let mut s = toml_session("a = 1\n[b]\nc = 2\nd = 3\n");
    s.expand_all();
    let a = vec![Seg::Key("a".into())];
    let b = vec![Seg::Key("b".into())];
    let c = vec![Seg::Key("b".into()), Seg::Key("c".into())];
    let d = vec![Seg::Key("b".into()), Seg::Key("d".into())];

    // Mid-band on an expanded, non-inline branch -> Into.
    assert_eq!(s.pointer_slot(&b, 0.5), Some(PasteSlot::Into(b.clone())));
    // Bottom band on a leaf -> After(that leaf).
    assert_eq!(s.pointer_slot(&a, 0.9), Some(PasteSlot::After(a.clone())));
    // Top band on `b`'s first child `c` -> After(b) (== "first child of b",
    // exactly `c`'s own position, via `resolve_target`'s expanded-branch rule).
    assert_eq!(s.pointer_slot(&c, 0.1), Some(PasteSlot::After(b.clone())));
    // Top band on `b`'s second child `d` -> After(c): here the preceding
    // flattened slot and "previous sibling" happen to coincide (`c` is a
    // leaf) — the differentiating case is below.
    assert_eq!(s.pointer_slot(&d, 0.1), Some(PasteSlot::After(c.clone())));
    // Unknown path -> None.
    assert_eq!(s.pointer_slot(&vec![Seg::Key("nope".into())], 0.5), None);
}

#[test]
fn pointer_slot_top_band_skips_into_an_expanded_previous_sibling() {
    // `r`'s previous *sibling* is `s`, an expanded branch with children `x`,
    // `y`. The preceding *flattened* slot before `r` is After(y) (s's last
    // child) — landing visually between `s`'s subtree and `r`, exactly where
    // the top-band click pointed. A sibling-position shortcut would wrongly
    // return After(s), which `slot_target` resolves to "prepend into s's
    // children" (`resolve_target`'s expanded-branch rule) — deep inside s's
    // subtree, nowhere near where the user clicked. This is the regression
    // guard for that bug.
    let mut s = toml_session("[s]\nx = 1\ny = 2\n\nr = 3\n");
    s.expand_all();
    let y = vec![Seg::Key("s".into()), Seg::Key("y".into())];
    let r = vec![Seg::Key("r".into())];
    assert_eq!(s.pointer_slot(&r, 0.1), Some(PasteSlot::After(y)));
}

#[test]
fn pointer_slot_withholds_into_for_a_single_line_inline_container() {
    let mut s = toml_session("t = { x = 1, y = 2 }\n");
    let t = vec![Seg::Key("t".into())];
    assert_eq!(
        s.tree.node_at(&t).map(|n| n.format),
        Some(Format::Inline)
    );
    // Mid-band would normally be Into, but a `Format::Inline` branch has no
    // "insert into" drop zone (mirrors the existing web `dnd.ts` comment) —
    // falls through to After.
    assert_eq!(s.pointer_slot(&t, 0.5), Some(PasteSlot::After(t.clone())));
}

#[test]
fn set_paste_slot_ignores_a_slot_whose_path_is_not_visible() {
    let mut s = toml_session("a = 1\n[b]\nc = 2\n");
    // `b` is collapsed by default; `c` is not visible.
    let c = vec![Seg::Key("b".into()), Seg::Key("c".into())];
    s.set_paste_slot(PasteSlot::After(c.clone()));
    assert_eq!(s.paste_slot, None);
    let a = vec![Seg::Key("a".into())];
    s.set_paste_slot(PasteSlot::After(a.clone()));
    assert_eq!(s.paste_slot, Some(PasteSlot::After(a)));
}

#[test]
fn dispatch_set_paste_slot_intent_arms_the_target_for_paste() {
    let mut s = toml_session("a = 1\n[b]\nc = 2\n");
    s.expand_all();
    let a = vec![Seg::Key("a".into())];
    let b = vec![Seg::Key("b".into())];
    s.cursor = a.clone();
    s.copy_selected();
    let snap = s.dispatch(Intent::SetPasteSlot(PasteSlot::Into(b.clone())));
    assert_eq!(snap.paste_slot, Some(PasteSlot::Into(b)));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test session_headless pointer_slot -- --nocapture`
Expected: FAIL to compile — no `pointer_slot`/`set_paste_slot` methods, no `Intent::SetPasteSlot` variant.

- [ ] **Step 3: Implement**

In `crates/confy-core/src/session/session.rs`, add right after `slot_target` (after its closing `}` at line 536):

```rust
    /// Pointer analogue of arrow-key `PasteSlot` stepping: turn "this row,
    /// this relative vertical position" (`0.0` = row top, `1.0` = row bottom)
    /// into a `PasteSlot`, so every pointer host (Web mouse click, touch tap,
    /// drag-drop into-eligibility) shares one target-classification algorithm
    /// instead of each hand-rolling its own 0.25/0.75 band threshold (ADR
    /// 0004 §1). `None` if `path` is not currently visible.
    ///
    /// Mid-band (`0.25..0.75`) on a branch whose `format != Format::Inline`
    /// (a single-line container has no meaningful "insert into" drop zone) ->
    /// `Into(path)`. Bottom band -> `After(path)`. Top band resolves to the
    /// slot immediately preceding this row's own slot(s) in `paste_slots()`'s
    /// flattened order — **not** a tree-sibling computation: for an expanded
    /// branch, `After(that branch)` means "its first child" (`resolve_target`),
    /// so the row before an expanded branch's *next sibling* is that branch's
    /// *last descendant*, not the branch itself. Reusing `paste_slots()`
    /// directly (rather than re-deriving sibling/parent indices by hand)
    /// keeps this provably in sync with the TUI's own arrow-key stepping.
    pub fn pointer_slot(&self, path: &Path, rel_y: f32) -> Option<PasteSlot> {
        let row = self
            .visible_nodes()
            .into_iter()
            .find(|r| &r.node.path == path)?;
        let into_eligible = row.node.is_branch() && row.node.format != Format::Inline;
        if into_eligible && rel_y > 0.25 && rel_y < 0.75 {
            return Some(PasteSlot::Into(path.clone()));
        }
        if rel_y >= 0.75 {
            return Some(PasteSlot::After(path.clone()));
        }
        let slots = self.paste_slots();
        let mine = if into_eligible {
            PasteSlot::Into(path.clone())
        } else {
            PasteSlot::After(path.clone())
        };
        let mine_idx = slots.iter().position(|s| *s == mine)?;
        Some(slots.get(mine_idx.wrapping_sub(1)).cloned().unwrap_or(mine))
    }

    /// Pointer analogue of the TUI's arrow-key `PasteSlot` stepping: set the
    /// armed clipboard's target directly (Web UI/touch `Intent::SetPasteSlot`,
    /// built from `pointer_slot`). No-op if the slot's path is not currently
    /// visible — mirrors `set_cursor`'s guard, so a stale click (row
    /// scrolled/collapsed away between the pointer event and dispatch) can't
    /// arm a target the tree no longer shows.
    pub fn set_paste_slot(&mut self, slot: PasteSlot) {
        let path = match &slot {
            PasteSlot::Into(p) | PasteSlot::After(p) => p,
        };
        let visible = self.visible_nodes().iter().any(|r| &r.node.path == path);
        if visible {
            self.paste_slot = Some(slot);
        }
    }
```

In `crates/confy-core/src/session/intent.rs`, add right after `SetCursor(crate::model::node::Path),` (in the "Pointer (Web UI)" section):

```rust
    /// Pointer analogue of the TUI's arrow-key `PasteSlot` stepping (ADR 0004
    /// §1): set the armed clipboard's target directly. Built from
    /// `ConfySession::pointer_slot(path, rel_y)`; ignored if the path isn't
    /// currently visible when it lands (mirrors `SetCursor`).
    SetPasteSlot(crate::session::state::PasteSlot),
```

In `crates/confy-core/src/session/dispatch.rs`, in `apply()`'s "---- Pointer (Web UI) ----" section, add right after the `Intent::SetCursor(path) => self.set_cursor(path),` line:

```rust
            Intent::SetPasteSlot(slot) => self.set_paste_slot(slot),
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test session_headless pointer_slot -- --nocapture` and `cargo test --test session_headless set_paste_slot -- --nocapture` and `cargo test --test session_headless dispatch_set_paste_slot -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/confy-core/src/session/session.rs crates/confy-core/src/session/intent.rs crates/confy-core/src/session/dispatch.rs crates/confy-core/tests/session_headless.rs
git commit -m "feat(core): Session::pointer_slot + SetPasteSlot intent (ADR 0004 §1)"
```

---

### Task 3: `move_selection_to` gains `cut: bool`; `Intent::MoveSelectionTo` gains an optional `cut` field

**Files:**
- Modify: `crates/confy-core/src/session/clipboard.rs:110-154` (`move_selection_to`)
- Modify: `crates/confy-core/src/session/intent.rs` (`MoveSelectionTo` variant)
- Modify: `crates/confy-core/src/session/dispatch.rs:93-97` (`apply()` match arm)
- Modify: `web/types.ts` (`Intent.MoveSelectionTo`)
- Test: `crates/confy-core/tests/session_headless.rs`

**Interfaces:**
- Produces: `Session::move_selection_to(&mut self, sources: Vec<Path>, target: Path, index: usize, cut: bool)`. Existing behavior (`cut: true`) is the default when the wire field is omitted.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn move_selection_to_with_cut_false_copies_instead_of_moving() {
    let mut s = toml_session("[a]\nx = 1\n[b]\nc = 2\n");
    s.expand_all();
    let ax = vec![Seg::Key("a".into()), Seg::Key("x".into())];
    let b = vec![Seg::Key("b".into())];
    s.move_selection_to(vec![ax.clone()], b.clone(), 1, false);
    assert!(s.error.is_none(), "copy-drag should succeed: {:?}", s.error);
    // Source untouched (copy, not move).
    assert!(s.tree.node_at(&ax).is_some(), "source `a.x` must survive a copy-drag");
    // Destination gained the copy.
    let bx = vec![Seg::Key("b".into()), Seg::Key("x".into())];
    assert!(s.tree.node_at(&bx).is_some(), "`b` must gain a copy of `x`");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test session_headless move_selection_to_with_cut_false -- --nocapture`
Expected: FAIL to compile — `move_selection_to` takes 3 args, not 4.

- [ ] **Step 3: Implement**

In `crates/confy-core/src/session/clipboard.rs`, change the signature and the one `cut: true` literal (lines 115-139):

```rust
    /// Drag-reparent (Web UI): move (or, with `cut: false`, copy) `sources`
    /// into `target` at child `index`. Implemented as a one-shot cut/copy→paste
    /// so it reuses `do_paste`'s entire collision / illegal-destination /
    /// array-upgrade machinery (a real `Mutation::Move`/`Insert` under the
    /// hood) — the same primitive `Target` + `cut` -> `do_paste` a keyboard
    /// Copy → position → Paste sequence uses (ADR 0004 §1). A drop onto a
    /// source or into its own subtree is rejected; the document is untouched
    /// on any failure.
    pub fn move_selection_to(&mut self, sources: Vec<Path>, target: Path, index: usize, cut: bool) {
        if self.doc.is_none() {
            return;
        }
        let sources = crate::session::selection::normalize(sources);
        if sources.is_empty() {
            return;
        }
        if sources
            .iter()
            .any(|s| target == *s || (target.len() > s.len() && target.starts_with(s)))
        {
            self.error = Some(tr(self.lang, "core.move.self").to_string());
            return;
        }
        let doc = self.doc.as_ref().unwrap();
        let fragments: Vec<String> = sources
            .iter()
            .map(|p| doc.serialize_fragment_relative(p))
            .collect();
        let cb = Clipboard {
            fragments,
            cut,
            sources,
        };
        let tgt = Target {
            parent: target,
            index,
        };
```

(the rest of the function body — the `prev`/`do_paste`/restore-clipboard tail — is unchanged.)

In `crates/confy-core/src/session/intent.rs`, change `MoveSelectionTo` and add a default-fn helper near the top of the file (right after the `use serde::{Deserialize, Serialize};` line):

```rust
/// Serde default for `Intent::MoveSelectionTo.cut` — omitting the field on
/// the wire preserves the pre-ADR-0004 cut-only behavior.
fn default_move_cut() -> bool {
    true
}
```

and the variant itself:

```rust
    MoveSelectionTo {
        sources: Vec<crate::model::node::Path>,
        target: crate::model::node::Path,
        index: usize,
        /// Copy (`false`) vs move (`true`, the default). A drag-drop with the
        /// platform copy modifier (⌥/Ctrl) held sends `false`; a plain
        /// drag-drop omits it (ADR 0004 §1).
        #[serde(default = "default_move_cut")]
        cut: bool,
    },
```

In `crates/confy-core/src/session/dispatch.rs`, update the match arm:

```rust
            Intent::MoveSelectionTo {
                sources,
                target,
                index,
                cut,
            } => self.move_selection_to(sources, target, index, cut),
```

In `web/types.ts`, update the `Intent` union member:

```ts
  | { MoveSelectionTo: { sources: Path[]; target: Path; index: number; cut?: boolean } }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test session_headless move_selection_to_with_cut_false -- --nocapture`
Expected: PASS

- [ ] **Step 5: Run the full existing drag-move regression suite (no behavior change for `cut: true` callers)**

Run: `cargo test --test session_headless dispatch_move -- --nocapture` and `cargo test --test session_headless dispatch_paste -- --nocapture`
Expected: PASS (unchanged)

- [ ] **Step 6: Commit**

```bash
git add crates/confy-core/src/session/clipboard.rs crates/confy-core/src/session/intent.rs crates/confy-core/src/session/dispatch.rs web/types.ts crates/confy-core/tests/session_headless.rs
git commit -m "feat(core): move_selection_to cut:bool param; MoveSelectionTo.cut wire field (ADR 0004 §1)"
```

---

### Task 4: wasm export + `web/confy.ts` wrapper + full `web/types.ts` sync

**Files:**
- Modify: `crates/confy-ffi/src/lib.rs`
- Modify: `web/confy.ts`
- Modify: `web/types.ts`

**Interfaces:**
- Produces: `ConfySession.pointer_slot(path, rel_y) -> PasteSlot | undefined` (wasm), `Session.pointerSlot(path, relY): PasteSlot | undefined` (TS wrapper), `web/types.ts` exports `PasteSlot` and adds it to `SessionSnapshot`/`Intent`.

- [ ] **Step 1: Implement the wasm export**

In `crates/confy-ffi/src/lib.rs`, add to `ConfySession`'s `impl` block, right after `children` (after its closing `}`):

```rust
    /// Pointer-drop classification (Web mouse / touch): "this row, this
    /// relative vertical position" -> the `PasteSlot` it represents, or
    /// `undefined` if the row is no longer visible. Every pointer surface
    /// (click-to-target while armed, drag-drop into/before/after
    /// eligibility) calls this instead of hand-rolling the classification
    /// (ADR 0004 §1).
    pub fn pointer_slot(&self, path: JsValue, rel_y: f32) -> Result<JsValue, JsValue> {
        let path: Path = from_value(path).map_err(js_serde_error)?;
        match self.session.pointer_slot(&path, rel_y) {
            Some(slot) => to_value(&slot).map_err(js_serde_error),
            None => Ok(JsValue::UNDEFINED),
        }
    }
```

- [ ] **Step 2: Build to verify it compiles**

Run: `cargo build -p confy-ffi --target wasm32-unknown-unknown 2>&1 | tail -30`
Expected: builds clean (or, if the wasm target isn't installed locally, `cargo check -p confy-ffi 2>&1 | tail -30` as a native-target syntax/type check substitute).

- [ ] **Step 3: Add the TS wrapper method**

In `web/confy.ts`, add to the `Session` class right after `children`:

```ts
  /**
   * Pointer-drop classification (ADR 0004 §1): "this row, this relative
   * vertical position" (`0` = row top, `1` = row bottom) -> the `PasteSlot`
   * it represents, or `undefined` if the row is no longer visible.
   */
  pointerSlot(path: Path, relY: number): PasteSlot | undefined {
    return this.raw.pointer_slot(path, relY) as PasteSlot | undefined;
  }
```

and add `PasteSlot` to the `import type { ... } from "./types.js";` list.

- [ ] **Step 4: Sync `web/types.ts`**

Add the `PasteSlot` type right after the `SessionSnapshot` interface's closing brace (before the `---- Intent ----` section comment):

```ts
// ---- PasteSlot (session::state::PasteSlot, ADR 0004 §1) ----
export type PasteSlot = { Into: Path } | { After: Path };
```

Add the field to `SessionSnapshot` (right after `clipboard_paths`):

```ts
  paste_slot: PasteSlot | undefined; // the armed clipboard's target (undefined when unarmed)
```

Add the variant to the `Intent` union (right after `{ SetCursor: Path }`):

```ts
  | { SetPasteSlot: PasteSlot }
```

- [ ] **Step 5: Verify the TS compiles**

Run: `cd web && npx tsc --noEmit 2>&1 | tail -40` (or the project's existing type-check script if `package.json` defines one — check first: `cat web/package.json | grep -A3 '"scripts"'`)
Expected: no new errors.

- [ ] **Step 6: Commit**

```bash
git add crates/confy-ffi/src/lib.rs web/confy.ts web/types.ts
git commit -m "feat(web): expose pointer_slot over wasm; sync PasteSlot into types.ts (ADR 0004 §1)"
```

---

## Phase 2: AoT atomic-move fix (§3) + `CONTEXT.md` sync (§4)

### Task 5: Atomic move for an AoT entry into another `[A/T]` group/array

**Files:**
- Modify: `crates/confy-core/src/model/cst_edit/aot_group.rs` (replace `aot_entry_member_fragments`'s body with a shared walk + add `aot_entry_section_body`)
- Modify: `crates/confy-core/src/model/cst_edit/move_paste.rs` (`move_nodes`)
- Test: `crates/confy-core/tests/session_headless.rs`

**Interfaces:**
- Produces: `enum EntryEvent { Header(Vec<Seg>), Entry(String) }` + `fn walk_aot_entry_body(tree: &SyntaxNode, header: &SyntaxNode) -> Result<Vec<EntryEvent>, MutateError>` (both private to `aot_group.rs`) and `pub(crate) fn aot_entry_section_body(tree: &SyntaxNode, header: &SyntaxNode) -> Result<String, MutateError>`. `aot_entry_member_fragments`'s signature is unchanged — only its body becomes a thin formatter over `walk_aot_entry_body`, so `move_paste.rs`'s existing call site needs no edit.
- Consumes (unchanged): `header_path`, `aot_entry_end`, `path_key_display`, `aot_group_insert`, `MutateError`, `taplo::syntax::{SyntaxKind, SyntaxNode}`, `taplo::rowan::NodeOrToken` — all already imported in both files.

- [ ] **Step 1: Write the failing test**

Add to `crates/confy-core/tests/session_headless.rs` (needs `Format` already added to imports in Task 2):

```rust
#[test]
fn move_aot_entry_into_another_group_preserves_nested_section() {
    let mut s = toml_session(
        "[[fruit]]\nname = \"apple\"\n\n[fruit.physical]\ncolor = \"red\"\n\n[[items]]\nname = \"seed\"\n",
    );
    let fruit0 = vec![Seg::Key("fruit".into()), Seg::Index(0)];
    s.cursor = fruit0.clone();
    s.cut_selected();
    assert!(s.error.is_none(), "cut should succeed: {:?}", s.error);

    let items = vec![Seg::Key("items".into())];
    s.paste_slot = Some(PasteSlot::Into(items.clone()));
    s.paste();
    assert!(s.error.is_none(), "paste should succeed: {:?}", s.error);

    // `physical` must survive as a real nested table (`[T/S]`, `Format::Scope`)
    // under the new `items[1]` entry, not flattened to a dotted
    // `items[1].physical.color` key (the ADR 0004 §3 bug).
    let entry1 = vec![Seg::Key("items".into()), Seg::Index(1)];
    let mut physical = entry1.clone();
    physical.push(Seg::Key("physical".into()));
    let node = s
        .tree
        .node_at(&physical)
        .expect("nested `physical` table survives the atomic move");
    assert_eq!(node.format, Format::Scope, "sub-section stays a real nested table");

    let mut color = physical.clone();
    color.push(Seg::Key("color".into()));
    assert_eq!(
        s.tree.node_at(&color).and_then(|n| n.value.clone()),
        Some("\"red\"".to_string())
    );

    // Moved (cut), so `fruit` no longer has the entry.
    assert!(s.tree.node_at(&fruit0).is_none(), "cut removed the source entry");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test session_headless move_aot_entry_into_another_group_preserves_nested_section -- --nocapture`
Expected: FAIL — `node.format` is `Format::Dotted`, not `Format::Scope` (today's flatten-to-dotted behavior), or the assertion on `color`'s path fails since it currently projects as `items[1].physical.color` collapsed into a `[T/D]` node rather than a real nested `[items.physical]` scope.

- [ ] **Step 3: Implement**

In `crates/confy-core/src/model/cst_edit/aot_group.rs`, replace the whole `aot_entry_member_fragments` function (both share the identical `header_path`/`aot_entry_end`/`children_with_tokens` walk and the identical `TABLE_ARRAY_HEADER`/`TABLE_HEADER`/`ENTRY` match — the only difference is what each does with a header event and an entry event — so both now sit on one shared walk instead of duplicating it) with:

```rust
/// One member event within a `[[…]]` entry's body span: a nested `[table]`
/// header (path relative to the entry's own group-path ancestor — empty
/// means "back at the entry's own top level") or a `key = value` entry
/// line's own source text. Shared by `aot_entry_member_fragments` (flattens
/// to dotted form for a table/root destination) and `aot_entry_section_body`
/// (keeps headers verbatim for an atomic AoT/array-destination move, ADR
/// 0004 §3) — the two output shapes each insert engine needs, over the
/// identical tree walk.
enum EntryEvent {
    Header(Vec<Seg>),
    Entry(String),
}

fn walk_aot_entry_body(tree: &SyntaxNode, header: &SyntaxNode) -> Result<Vec<EntryEvent>, MutateError> {
    let group_path = header_path(header);
    let i = header.index();
    let end = aot_entry_end(tree, &group_path, i);
    let els: Vec<_> = tree.children_with_tokens().collect();
    let mut events = Vec::new();
    for el in &els[i + 1..end] {
        if let NodeOrToken::Node(n) = el {
            match n.kind() {
                SyntaxKind::TABLE_ARRAY_HEADER => return Err(MutateError::Unsupported),
                SyntaxKind::TABLE_HEADER => {
                    events.push(EntryEvent::Header(header_path(n)[group_path.len()..].to_vec()));
                }
                SyntaxKind::ENTRY => {
                    events.push(EntryEvent::Entry(n.to_string().trim().to_string()));
                }
                _ => {}
            }
        }
    }
    Ok(events)
}

pub(crate) fn aot_entry_member_fragments(
    tree: &SyntaxNode,
    header: &SyntaxNode,
) -> Result<Vec<String>, MutateError> {
    let mut prefix = String::new();
    let mut frags = Vec::new();
    for ev in walk_aot_entry_body(tree, header)? {
        match ev {
            EntryEvent::Header(rel) => prefix = path_key_display(&rel),
            EntryEvent::Entry(text) => frags.push(if prefix.is_empty() {
                format!("{text}\n")
            } else {
                format!("{prefix}.{text}\n")
            }),
        }
    }
    Ok(frags)
}

/// The full body of the `[[…]]` AoT entry backed by `header`, preserving
/// nested `[table]` sub-sections as *relative* headers (`[physical]`,
/// stripped of the entry's own group-path ancestor) instead of flattening
/// them to dotted keys — used when the destination is itself an `[A/T]`
/// group or array (ADR 0004 §3), so the caller's `insert`/`prefix_section_headers`
/// pass re-qualifies each relative header against the *destination's* key,
/// reconstructing the same nested structure atomically instead of losing it
/// to a dotted-key rewrite. `Err(Unsupported)` on a nested `[[…]]` sub-group,
/// same as `aot_entry_member_fragments` (it has no dotted/atomic form either).
pub(crate) fn aot_entry_section_body(tree: &SyntaxNode, header: &SyntaxNode) -> Result<String, MutateError> {
    let mut body = String::new();
    for ev in walk_aot_entry_body(tree, header)? {
        match ev {
            EntryEvent::Header(rel) => body.push_str(&format!("[{}]\n", path_key_display(&rel))),
            EntryEvent::Entry(text) => {
                body.push_str(&text);
                body.push('\n');
            }
        }
    }
    Ok(body)
}
```

`Seg` must be in scope in `aot_group.rs` for `EntryEvent::Header(Vec<Seg>)` — check its existing imports first (`grep -n "^use" crates/confy-core/src/model/cst_edit/aot_group.rs`); add `use crate::model::node::Seg;` (or extend an existing `crate::model::node::{...}` import) if it isn't already imported.

In `crates/confy-core/src/model/cst_edit/move_paste.rs`:

Add `use std::collections::HashSet;` near the top (after the existing `use` block, e.g. right before `use crate::model::document::...`).

Change the `aot_group` import (currently `use super::aot_group::{aot_entry_member_fragments, aot_group_insert};`) to:

```rust
use super::aot_group::{aot_entry_member_fragments, aot_entry_section_body, aot_group_insert};
```

Replace the whole `move_nodes` function body (from `let (proj, idx) = walk(tree, "");` through the final `Ok(())`) with:

```rust
    let (proj, idx) = walk(tree, "");

    // Destination `[A/T]` group or plain array: computed up front (before the
    // capture loop) since `Target::AotEntry` sources need to know it to
    // decide between flattening (table/root dest, unchanged) and the atomic
    // reconstruction below (ADR 0004 §3) — `target.parent`'s kind doesn't
    // depend on `frags`.
    let dest_packs = node_at(&proj.root, &target.parent)
        .is_some_and(|n| matches!(n.kind, NodeKind::ArrayOfTables | NodeKind::Array));

    // Capture each source's source text before any removal.
    let mut frags: Vec<String> = Vec::new();
    // Indices into `frags` whose text is a composite AoT-entry body carrying
    // its own nested `[table]` headers (ADR 0004 §3) — these splice via
    // `aot_group_insert` directly, bypassing `insert`'s generic
    // `has_header -> Illegal` gate (correct for a *bare* section paste, wrong
    // here since this is an entry's *own* body with header-qualified
    // sub-sections `insert`'s `prefix_section_headers` pass re-prefixes
    // against the destination). Stays valid across the `dest_packs`-join
    // below: a composite always contains a header, so it always fails
    // `joinable_entry` and is never touched by that join.
    let mut aot_composite_idxs: HashSet<usize> = HashSet::new();
    for p in sources {
        // A table — `[T/D]`, `[T/S]` (scattered or not), implicit, or mixed — is an
        // open set of member spans: capture them all, scope-relative (entry keys
        // drop the headerless-ancestor prefix, headers drop the ancestor path), so
        // the re-insert re-prefixes only for the destination. A pure `[T/D]` table
        // fans out to one fragment per member line so the per-leaf collision check
        // applies; a sectioned capture stays one fragment (its entries belong under
        // their headers). The source side is removed by `delete`, which fans out
        // over the same spans.
        if node_at(&proj.root, p).is_some_and(|n| matches!(n.kind, NodeKind::Table))
            && matches!(p.last(), Some(Seg::Key(_)))
        {
            let spans = table_member_spans(tree, &idx, p);
            if spans.iter().any(|s| matches!(s, MemberSpan::Section(_))) {
                if let Some(text) = table_fragment(tree, &idx, &proj.root, p, true) {
                    frags.push(text);
                    continue;
                }
            } else if !spans.is_empty() {
                let strip = dotted_ancestor_prefix_len(&idx, &proj.root, p);
                for s in &spans {
                    if let MemberSpan::Entry(m) = s {
                        frags.push(strip_key_prefix(m, strip));
                    }
                }
                continue;
            } else if let Some(inline_len) = inline_ancestor_len(&proj.root, p) {
                // A synthetic `[T/D]` table *inside an inline table* fans out to
                // its `{ … }` member entries, captured scope-relative (drop the
                // segments between the inline table and the node, keep its own
                // key) — the source side is removed by `delete`'s inline fan-out.
                let members = inline_member_entries(&idx, p);
                if !members.is_empty() {
                    let strip = p.len() - 1 - inline_len;
                    for m in &members {
                        frags.push(format!("{}\n", strip_key_prefix(m, strip).trim()));
                    }
                    continue;
                }
            }
        }
        let t = match idx.iter().find(|(ip, _)| ip == p).map(|(_, t)| t.clone()) {
            Some(t) => t,
            None => return Err(MutateError::NotFound),
        };
        match t {
            // Scope-relative capture: drop the source's dotted-ancestor prefix so the
            // re-insert re-prefixes only for the destination (matching copy/paste).
            Target::Entry(n) => {
                let strip = dotted_ancestor_prefix_len(&idx, &proj.root, p);
                frags.push(strip_key_prefix(&n, strip));
            }
            Target::Header(h) => frags.push(section_text(tree, p, h.index(), false)),
            // Moving an array element out: into another array it stays a bare element;
            // into a table/root an inline table `{ k = v, … }` **unpacks** into its
            // member entries (keys preserved, one node each — the per-leaf collision
            // check applies), anything else gets a synthesized `placeholder` key on
            // insert. The destination format is then applied by `insert` (dotted prefix,
            // inline-table splice, …).
            Target::ArrayElement(value) => {
                let text = value.to_string();
                let dest_is_array = node_at(&proj.root, &target.parent)
                    .map(|n| matches!(n.kind, crate::model::node::NodeKind::Array))
                    .unwrap_or(false);
                match (dest_is_array, unpack_inline_table(&text)) {
                    (false, Some(entries)) => frags.extend(entries),
                    _ => frags.push(format!("{}\n", text.trim())),
                }
            }
            // Moving a `[[…]]` entry out of its array: into a table/root it still
            // splits into member nodes (unchanged, `[T/D]`-parity — one fragment
            // per line, sub-sections flattened to dotted). Into another `[A/T]`
            // group or array it now moves *atomically* instead (ADR 0004 §3): the
            // body keeps its nested `[table]` sub-sections as relative headers
            // (`aot_entry_section_body`) rather than flattening them, and
            // `insert`'s existing `prefix_section_headers` re-qualifies them
            // against the destination once this composite reaches it below.
            Target::AotEntry(h) => {
                if dest_packs {
                    aot_composite_idxs.insert(frags.len());
                    frags.push(aot_entry_section_body(tree, &h)?);
                } else {
                    frags.extend(aot_entry_member_fragments(tree, &h)?);
                }
            }
            _ => return Err(MutateError::Unsupported),
        }
    }

    // Destination `[A/T]` group or plain array: several moved nodes pack into ONE
    // new `[[…]]` entry / `{ … }` element, so join the fragments when every one is
    // a header-less keyed entry (bare values / sections keep the per-fragment path
    // and its own handling). A composite AoT-entry body (above) always contains a
    // header, so it always fails `joinable_entry` and is never swept into this join
    // — it keeps its own slot, indices in `aot_composite_idxs` stay valid.
    let frags = if dest_packs && frags.len() > 1 && frags.iter().all(|f| joinable_entry(f)) {
        vec![frags
            .iter()
            .map(|f| format!("{}\n", f.trim_end()))
            .collect::<String>()]
    } else {
        frags
    };

    // Resolve a stable anchor — the first child at/after the target index that is
    // not itself a source *and not a comment* (a comment's positional path is not
    // stable across the source removals, so it can't be relocated by path) — to
    // insert before; its keyed path is stable. `None` means append.
    //
    // Because the anchor skips comment slots, comments sitting between
    // `target.index` and the anchor would otherwise be jumped over (the insert
    // landing *after* a trailing comment instead of at the requested slot). Count
    // those non-source comment slots as `gap` and subtract it from the relocated
    // anchor position so the insert lands at the intended ordinal.
    let parent = node_at(&proj.root, &target.parent).ok_or(MutateError::NotFound)?;
    let anchor_orig = parent
        .children
        .iter()
        .enumerate()
        .skip(target.index)
        .find(|(_, c)| {
            !matches!(c.kind, crate::model::node::NodeKind::Comment(_))
                && !sources.contains(&c.path)
        });
    let anchor_path: Option<Vec<Seg>> = anchor_orig.map(|(_, c)| c.path.clone());
    let anchor_end = anchor_orig.map_or(parent.children.len(), |(i, _)| i);
    let gap = parent.children[target.index.min(parent.children.len())..anchor_end]
        .iter()
        .filter(|c| !sources.contains(&c.path))
        .count();

    // Delete sources (longest path first keeps shallower paths valid).
    let mut ordered: Vec<&Vec<Seg>> = sources.iter().collect();
    ordered.sort_by_key(|p| std::cmp::Reverse(p.len()));
    for p in ordered {
        delete(tree, p)?;
    }

    // Re-insert before the anchor's current position (or append), in order.
    for (i, frag) in frags.into_iter().enumerate() {
        let (proj2, idx2) = walk(tree, "");
        let parent2 = node_at(&proj2.root, &target.parent).ok_or(MutateError::NotFound)?;
        let index = {
            let base = match &anchor_path {
                Some(ap) => parent2
                    .children
                    .iter()
                    .position(|c| &c.path == ap)
                    .unwrap_or(parent2.children.len()),
                None => parent2.children.len(),
            };
            base - gap.min(base)
        };
        if aot_composite_idxs.contains(&i) {
            let parsed = taplo::parser::parse(&frag);
            if let Some(e) = parsed.errors.first() {
                return Err(MutateError::Fragment(e.to_string()));
            }
            let node = parsed.into_syntax().clone_for_update();
            aot_group_insert(tree, &idx2, parent2, &target.parent, index, &node, on_collision)?;
        } else {
            insert(
                tree,
                &InsTarget {
                    parent: target.parent.clone(),
                    index,
                },
                &frag,
                on_collision,
            )?;
        }
    }
    Ok(())
```

Also update the function's stale doc comment (immediately above `pub(crate) fn move_nodes`) — replace:

```rust
/// Move `sources` to `target`, atomically (the caller commits the clone only on
/// success). Comments are independent CST nodes, so a move repositions only the
/// named nodes - adjacent comments stay put with no special handling. Entry,
/// `[table]` and **array-element** sources are supported; AoT-entry sources are
/// deferred (they would need append-not-collide insert semantics for `[[x]]`).
```

with:

```rust
/// Move `sources` to `target`, atomically (the caller commits the clone only on
/// success). Comments are independent CST nodes, so a move repositions only the
/// named nodes - adjacent comments stay put with no special handling. Entry,
/// `[table]` and **array-element** sources are supported. An AoT-entry source
/// (`Target::AotEntry`) splits into dotted member fragments for a table/root
/// destination, or moves atomically (nested sections preserved) into another
/// `[A/T]` group/array — a nested `[[…]]` sub-group has neither form
/// (`Unsupported`, ADR 0004 §3).
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test session_headless move_aot_entry_into_another_group_preserves_nested_section -- --nocapture`
Expected: PASS

- [ ] **Step 5: Run the full existing AoT-entry regression suite (unchanged table/root-destination case)**

Run: `cargo test -p confy-core 2>&1 | tail -40`
Expected: all passing, 0 failed (this touches shared insertion/move machinery — confirm no regressions across the whole crate).

- [ ] **Step 6: Commit**

```bash
git add crates/confy-core/src/model/cst_edit/aot_group.rs crates/confy-core/src/model/cst_edit/move_paste.rs crates/confy-core/tests/session_headless.rs
git commit -m "fix(core): AoT entry -> AoT/array destination moves atomically, preserving nested sections (ADR 0004 §3)"
```

---

### Task 6: `CONTEXT.md` + `CHANGELOG.md` sync (§4)

**Files:**
- Modify: `CONTEXT.md:294-299` (Insert / move legality note)
- Modify: `CONTEXT.md:338` (Mutation mechanics → Move row)
- Modify: `CONTEXT.md:202-209` (`PasteSlot` glossary entry's `After` wording)
- Modify: `CHANGELOG.md`

**Interfaces:** none (documentation only).

- [ ] **Step 1: Update the Insert / move legality note**

Re-read `CONTEXT.md:290-300` first to get the current line numbers/tag (edits since Task 5 don't touch this file, but always re-anchor before a hash-line edit), then replace the note (currently starting `- ⏸ = an **AoT *group***...`):

```markdown
- ⏸ = an **AoT *group*** as a whole-group source is `Unsupported` for move (and degrades for
  copy). An AoT ***entry*** (`product[0]`) move/copy **works**: into a table/root it splits into
  member fragments (`aot_entry_member_fragments`, sub-sections flattened to dotted entries) and
  lands as nodes (dotted re-prefix, per-leaf collision); into another `[A/T]` group or array it
  moves **atomically** — its own body lines land directly and every nested `[table]` sub-section
  is reconstructed as a nested section under the new entry (`aot_entry_section_body` +
  `prefix_section_headers`, ADR 0004 §3), not flattened to a dotted key. A nested `[[…]]`
  sub-group has no dotted/atomic form either way: move → `Unsupported`, copy → full-section
  capture.
```

- [ ] **Step 2: Update the Mutation mechanics → Move row**

Replace the `| **Move** | ... |` row:

```markdown
| **Move** | Atomic: delete-before-reinsert on a scratch tree, committed only on success, so a same-scope reposition is a move, not a `Key already exists` collision. An `[A/T]` *entry* moved/copied out of its array splits into member fragments; into a table/root the body lines land as nodes (**sub-sections flattened to dotted**: `[fruit.physical]` `color` → `physical.color`, `aot_entry_member_fragments`); into another `[A/T]` group or array it moves **atomically** instead — nested `[table]` sub-sections are reconstructed as nested sections under the new entry, not flattened (`aot_entry_section_body`, ADR 0004 §3). A whole-`[A/T]`-*group* Move degrades to `Unsupported`. A nested `[[…]]` sub-group has no dotted/atomic form either way: Move → `Unsupported`, Copy → full-section capture. |
```

- [ ] **Step 3: Fix the existing `PasteSlot` glossary entry's imprecise `After` description**

Grilling this plan surfaced that `CONTEXT.md`'s already-shipped `PasteSlot` glossary entry (from
the prior ADR 0004 grilling session) states `After` "inserts as its next sibling" — true only for
a leaf or collapsed branch. For an **expanded branch**, `slot_target`'s actual `After(p)`
resolution (`session.rs:524-533`, via `resolve_target`) is "p's first child" instead — the exact
fact `pointer_slot`'s top-band case (Task 2) depends on. Re-read `CONTEXT.md:200-210` first to
confirm current line numbers/tag, then replace the entry (currently starting `**PasteSlot**
(\`Into(Path)\` / \`After(Path)\`):`):

```markdown
**PasteSlot** (`Into(Path)` / `After(Path)`):
The target of an armed clipboard (copy/cut) — a navigable gap-cursor distinct from the tree
cursor. `Into` a branch appends as its last child. `After` a Node inserts as its next sibling —
except an **expanded branch**, where it inserts as that branch's first child instead (so
`Into`-then-`After` on the same expanded branch land adjacently, matching how `paste_slots()`
flattens the tree; see `resolve_target`). Canonical, cross-platform vocabulary for "where a
paste/move lands" — not TUI-only, even though the TUI was the first surface to navigate and
render it (arrow keys step through the flattened `Into`-then-`After` sequence; ADR 0004).
_Avoid_: drop target, insertion point (these describe the visual affordance, not the domain
concept).
```

- [ ] **Step 4: Add the CHANGELOG entry**

Read `CHANGELOG.md`'s `## [Unreleased]` → `### Fixed`/`### Added` sections for current line numbers, then add (single line, matching the file's convention):

```markdown
- fix(core): moving/copying an `[[array-of-tables]]` entry into another `[A/T]` group or array now moves atomically — nested `[table]` sub-sections (`[fruit.physical]`) are reconstructed as nested sections under the destination entry instead of flattening to a dotted key (ADR 0004 §3, `aot_entry_section_body`).
- feat(core): `PasteSlot` (`Into`/`After`) is now the shared target representation for every host — new `SessionSnapshot.paste_slot`, `Intent::SetPasteSlot`, `Session::pointer_slot(path, rel_y)` (pixel-position → target), `move_selection_to` gains `cut: bool` (ADR 0004 §1).
```

- [ ] **Step 5: Verify — no build step (docs only); grep to confirm all three edits landed**

Run: `grep -n "aot_entry_section_body" CONTEXT.md && grep -n "first child instead" CONTEXT.md`
Expected: 2 matches for `aot_entry_section_body` (the legality note, the Move row) and 1 match for `first child instead` (the glossary fix).

- [ ] **Step 6: Commit**

```bash
git add CONTEXT.md CHANGELOG.md
git commit -m "docs: sync CONTEXT.md AoT-entry move description with the atomic-move fix (ADR 0004 §4)"
```

---

## Phase 3: Web + touch pointer wiring + rendering

### Task 7: `web/ui.ts` — click-while-armed calls `pointerSlot` instead of bare `SetCursor`

**Files:**
- Modify: `web/ui.ts` (`onTreeClick`, `focusRow`)

**Interfaces:**
- Consumes: `Session.pointerSlot` (Task 4), `Intent.SetPasteSlot` (Task 2/4).

- [ ] **Step 1: Add the shared helper and wire both call sites**

Add a new function right above `focusRow` (before line 1030's comment block):

```ts
// Pointer analogue of arrow-key `PasteSlot` stepping (ADR 0004 §1): while the
// clipboard is armed, a click positions the paste target precisely
// (`Into`/`After`) from the click's row + relative vertical position, instead
// of always falling back to the bare cursor (`After(cursor)`). Shared by
// `onTreeClick`'s plain row-body branch and `focusRow` (caret/kind-badge/
// edit-cell affordance clicks), so every click while armed narrows the target
// the same way.
function armedPasteTarget(path: Path, ev: MouseEvent): Intent {
  const rowEl = (ev.target as HTMLElement).closest(".row") as HTMLElement | null;
  if (rowEl && session) {
    const r = rowEl.getBoundingClientRect();
    const relY = (ev.clientY - r.top) / (r.height || 1);
    const slot = session.pointerSlot(path, relY);
    if (slot) return { SetPasteSlot: slot };
  }
  return { SetCursor: path };
}
```

Replace `focusRow`'s clipboard-armed branch (line 1039):

```ts
function focusRow(path: Path, ev: MouseEvent) {
  if (!snap) return;
  if ((snap.clipboard_count ?? 0) > 0) return send(armedPasteTarget(path, ev));
  send({ SetSelection: { paths: resolveClick(snap, path, ev) } });
}
```

Replace `onTreeClick`'s clipboard-armed branch (lines 1118-1123):

```ts
  // In paste mode the clipboard freezes the selection, so a click can't
  // reselect — it positions the paste target (`Into`/`After`, from the
  // click's row-relative Y) instead, and `body.paste-mode` styling makes it
  // visible (ADR 0004 §1).
  if ((snap.clipboard_count ?? 0) > 0) {
    return send(armedPasteTarget(path, ev));
  }
```

- [ ] **Step 2: Type-check**

Run: `cd web && npx tsc --noEmit 2>&1 | tail -40`
Expected: no new errors.

- [ ] **Step 3: Commit**

```bash
git add web/ui.ts
git commit -m "feat(web): armed click positions PasteSlot precisely via pointerSlot (ADR 0004 §1)"
```

---

### Task 8: `web/render.ts` + `web/ui.ts` — Into/After visual cue for the armed paste target

**Files:**
- Modify: `web/render.ts` (`renderRow` signature + class string)
- Modify: `web/ui.ts` (`renderRawOrTree`/`render`, new `renderPasteSlotCue`)
- Test: `web/render.spec.mjs`

**Interfaces:**
- Consumes: `SessionSnapshot.paste_slot` (Task 1/4).
- Produces: `renderRow`'s 7th (optional) parameter `pasteInto: boolean`; `renderPasteSlotCue(snap: SessionSnapshot): void`.

- [ ] **Step 1: Write the failing spec test**

Add to `web/render.spec.mjs`, right after the existing two `renderRow` escaping checks (after the block ending `assertEscaped(html, VALUE_PAYLOAD, "comment-row value");` / its closing `}`):

```js
// ---- renderRow: Into-armed row gets the drag-over-into class (ADR 0004 §1) ----
console.log("\n-- renderRow(): paste-armed Into styling --");
{
  const row = makeRow({ key: "b", is_branch: true });
  const htmlPlain = renderRow(row, 0, [row], null, null, "");
  check("plain row has no drag-over-into class", !htmlPlain.includes("drag-over-into"));
  const htmlInto = renderRow(row, 0, [row], null, null, "", true);
  check("Into-armed row gets drag-over-into class", htmlInto.includes("drag-over-into"));
}
```

(If `makeRow` doesn't already accept an `is_branch` override, check its definition near the top of `render.spec.mjs` and extend it — it almost certainly does, since `ViewRow.is_branch` is a required field every fixture must set.)

- [ ] **Step 2: Run test to verify it fails**

Run: `cd web && node render.spec.mjs 2>&1 | tail -20`
Expected: FAIL — `renderRow` ignores a 7th argument today, `htmlInto` has no `drag-over-into` class.

- [ ] **Step 3: Implement `renderRow`'s new parameter**

In `web/render.ts`, change the signature (add a 7th, optional, defaulted parameter) and fold it into the class string:

```ts
export function renderRow(
  r: ViewRow,
  idx: number,
  rows: ViewRow[],
  edit: EditView | null,
  schemaEnum: { options: string[]; cursor: number } | null,
  clip: "" | " clip-copy" | " clip-cut",
  pasteInto: boolean = false,
): string {
  const pathAttr = escapeHtml(JSON.stringify(r.path));
  const comment = isCommentRow(r);
  const expanded = r.is_branch && isExpanded(rows, idx);
  const cls =
    `row${r.is_branch ? " branch" : ""}${expanded ? " open" : ""}` +
    `${r.is_cursor ? " cursor" : ""}${r.selected ? " selected" : ""}` +
    `${r.read_only ? " readonly" : ""}${comment ? " comment-row" : ""}${clip}` +
    `${r.violations ? " schema-violation" : ""}${pasteInto ? " drag-over-into" : ""}`;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd web && node render.spec.mjs 2>&1 | tail -20`
Expected: PASS

- [ ] **Step 5: Wire the call site + the After-slot line cue**

In `web/render.ts`, update the `renderRow(...)` call (inside the function that builds `next` — find it via the existing `renderRow(` call site) to pass the new argument:

```ts
      html: renderRow(
        r,
        idx,
        rows,
        edit,
        schemaEnum,
        clipKeys.has(JSON.stringify(r.path)) ? clipCls : "",
        pasteIntoPath !== null && JSON.stringify(r.path) === pasteIntoPath,
      ),
```

and compute `pasteIntoPath` right before the `rows.forEach` loop (next to the existing `clipKeys`/`clipCls` computation):

```ts
  // Armed-paste `Into` target, keyed so the loop above can compare per row
  // (ADR 0004 §1) — `After` is a cross-row line, drawn separately in
  // `ui.ts`'s `renderPasteSlotCue` since it isn't any single row's own class.
  const pasteIntoPath =
    snap.paste_slot && "Into" in snap.paste_slot ? JSON.stringify(snap.paste_slot.Into) : null;
```

In `web/ui.ts`, add a new function right after `renderRawOrTree` (after its closing `}`):

```ts
// The armed-paste `After` target renders as the same green insertion line
// drag-drop already uses (`#dropLine`) — reused rather than duplicated
// (ADR 0004 §1). `Into` is a per-row class, already baked into `renderRow`'s
// output by `render.ts`.
function renderPasteSlotCue(snap: SessionSnapshot) {
  const dropLine = $("dropLine");
  const slot = snap.paste_slot;
  if (!slot || !("After" in slot) || rawView) {
    dropLine.style.display = "none";
    return;
  }
  const rowEl = tree.querySelector<HTMLElement>(
    `.row[data-path='${CSS.escape(JSON.stringify(slot.After))}']`,
  );
  if (!rowEl) {
    dropLine.style.display = "none";
    return;
  }
  const wrap = $("treeWrap");
  const r = rowEl.getBoundingClientRect();
  const wr = wrap.getBoundingClientRect();
  const indentW = (rowEl.querySelector(".indent") as HTMLElement | null)?.offsetWidth ?? 0;
  dropLine.style.top = `${r.bottom - wr.top + wrap.scrollTop}px`;
  dropLine.style.left = `${indentW + 8}px`;
  dropLine.style.display = "block";
}
```

and call it from `render()` right after `renderRawOrTree();` (line 334):

```ts
  renderRawOrTree();
  renderPasteSlotCue(snap);
```

- [ ] **Step 6: Type-check + rerun the JS spec suite**

Run: `cd web && npx tsc --noEmit 2>&1 | tail -40 && node render.spec.mjs 2>&1 | tail -20`
Expected: no new type errors; spec suite green.

- [ ] **Step 7: Commit**

```bash
git add web/render.ts web/ui.ts web/render.spec.mjs
git commit -m "feat(web): Into/After visual cue for the armed paste target, reusing drag styling (ADR 0004 §1)"
```

---

### Task 9: `web/dnd.ts` — drag-drop's into-eligibility check calls `pointerSlot`

**Files:**
- Modify: `web/dnd.ts` (`installDnd` signature, `dragover` handler)
- Modify: `web/ui.ts` (`installDnd` call site)

**Interfaces:**
- Consumes: `Session.pointerSlot` (Task 4). Drag-drop's own before/after sibling-index `Target` math is untouched — only the "should I offer an Into drop" boolean moves to core, replacing the hand-rolled `vr?.is_branch && vr.format !== "Inline"` check (the exact drift point the ADR calls out, since `touch/app.ts`'s own copy of this check already diverged — see Task 10).

- [ ] **Step 1: Implement**

In `web/dnd.ts`, change `installDnd`'s signature to take a `pointerSlot` callback:

```ts
import type { Intent, Path, PasteSlot, SessionSnapshot, ViewRow } from "./types.js";
import { parentOf, pathEq as eq, siblingIndex } from "./path-utils.js";

type DropTarget =
  | { mode: "into"; path: Path }
  | { mode: "before" | "after"; path: Path };

export function installDnd(
  treeEl: HTMLElement,
  getSnap: () => SessionSnapshot | null,
  send: (i: Intent) => void,
  pointerSlot: (path: Path, relY: number) => PasteSlot | undefined,
): void {
```

Replace the `dragover` handler's into-eligibility check (the `if (vr?.is_branch && vr.format !== "Inline" && rel > 0.25 && rel < 0.75) {` line and its accompanying comment block):

```ts
    const isInto = (slot: PasteSlot | undefined): slot is { Into: Path } =>
      !!slot && "Into" in slot;
    // Into-eligibility (branch, non-`Format::Inline`) now comes from core's
    // `pointer_slot` (ADR 0004 §1) instead of a hand-rolled copy of the same
    // check — `touch/app.ts`'s own copy had already drifted (different
    // thresholds, no `Format::Inline` guard at all), which is exactly the
    // per-surface drift this unification eliminates. The before/after
    // sibling-index math below is unchanged — only the into/not-into decision
    // is now core's call.
    if (isInto(pointerSlot(path, rel))) {
      row.classList.add("drag-over-into");
      target = { mode: "into", path };
    } else {
```

(Keep the existing `else` block's body — the `before`/sibling-index computation — verbatim; only the `if` condition and its comment change. Delete the now-unused `vr` local if nothing else in the handler references it after this change — check with a grep first: `grep -n "\bvr\b" web/dnd.ts`.)

- [ ] **Step 2: Update the call site**

In `web/ui.ts`, update `installDnd(tree, () => snap, send);` (line 1649):

```ts
  installDnd(tree, () => snap, send, (p, r) => session!.pointerSlot(p, r));
```

- [ ] **Step 3: Type-check**

Run: `cd web && npx tsc --noEmit 2>&1 | tail -40`
Expected: no new errors.

- [ ] **Step 4: Manual smoke test (browser)**

Launch the web dev server (check `web/package.json` for the existing `dev`/`serve` script), open the app, drag a branch row's grip and hover its middle band — confirm the green `Into` outline still appears; hover its top/bottom band — confirm the `#dropLine` insertion line still appears at the right position; try the same on an inline-table row (`t = { x = 1 }`) and confirm the mid-band no longer offers `Into` (only before/after).

- [ ] **Step 5: Commit**

```bash
git add web/dnd.ts web/ui.ts
git commit -m "refactor(web): dnd.ts into-eligibility via core pointer_slot, drops hand-rolled Format check (ADR 0004 §1)"
```

---

### Task 10: `web/touch/app.ts` — tap-while-armed + reorder into-eligibility via `pointerSlot`

**Files:**
- Modify: `web/touch/app.ts` (`handleTap`, its call site, `onReorderMove`)

**Interfaces:**
- Consumes: `Session.pointerSlot` (Task 4, module-level `session` is already in scope in this file — confirmed by the existing `session.kindOptions(path)` call).

- [ ] **Step 1: Thread `clientY` into `handleTap`**

Update the call site (line 1100):

```ts
    } else if (dragging && dragRow && !moved) {
      handleTap(e.target as HTMLElement, dragRow, e.clientY);
    }
```

Update `handleTap`'s signature and its two clipboard-armed branches (lines 1140, 1156-1163, 1179-1182):

```ts
function handleTap(target: HTMLElement, row: HTMLElement, clientY: number) {
  const path = pathOf(row);
  if (!path) return;
  const armedTarget = (): Intent => {
    if (session) {
      const r = row.getBoundingClientRect();
      const relY = (clientY - r.top) / (r.height || 1);
      const slot = session.pointerSlot(path, relY);
      if (slot) return { SetPasteSlot: slot };
    }
    return { SetCursor: path };
  };
  const actBtn = target.closest<HTMLElement>("[data-act]");
  if (actBtn) {
    const act = actBtn.dataset.act;
    if (act === "grip") return;
    // Revealed Delete (swipe-to-delete): remove this row, then re-render closes it.
    if (act === "rowdel") {
      openSwipeMain = null;
      send({ SetCursor: path });
      send({ SetSelection: { paths: [path] } });
      const after = sendR("DeleteSelected");
      toast(after.error ?? "Deleted");
      return;
    }
    if (act === "caret") {
      // Paste mode freezes the selection (core's SetSelection is a no-op
      // there), so it falls back to positioning the paste target instead —
      // same guard as the plain-tap fallback below (ADR 0004 §1).
      if ((snap?.clipboard_count ?? 0) > 0) send(armedTarget());
      else selectOnly(path);
      return send("ToggleExpand");
    }
  }
  // A tap while a row is swiped open just closes it (no selection change).
  if (openSwipeMain) {
    const wasOpen = openSwipeMain;
    openSwipeMain.style.transform = "";
    openSwipeMain = null;
    setDelRevealed(wasOpen, false);
    if (wasOpen === row.querySelector(".row-main")) return;
  }
  const key = JSON.stringify(path);
  const now = Date.now();
  const isDouble = key === lastTapKey && now - lastTapTime < DOUBLE_TAP_MS;
  lastTapKey = key;
  lastTapTime = now;
  if (isDouble) openPanel(path);
  // In paste mode the clipboard freezes the selection, so a tap positions the
  // paste target (`Into`/`After`); `.app.paste-mode .row.cursor`/
  // `.row.drop-into` highlight it (ADR 0004 §1).
  else if ((snap?.clipboard_count ?? 0) > 0) send(armedTarget());
  else selectOnly(path);
}
```

- [ ] **Step 2: Fix `onReorderMove`'s into-eligibility check (also fixes the pre-existing threshold/Format drift)**

Replace the current classification block (lines 977-982):

```ts
  const hr = hit.getBoundingClientRect();
  if (!resolved) {
    const rel = (y - hr.top) / (hr.height || 1);
    const slot = session?.pointerSlot(pathOf(hit)!, rel);
    if (slot && "Into" in slot) {
      reMode = "into";
    } else {
      reMode = rel < 0.5 ? "before" : "after";
    }
  }
```

(This replaces the old `isBranch`/`0.28`/`0.72` hand-rolled thresholds — which never checked `Format::Inline` at all — with the same core classification `dnd.ts` now uses. `pathOf(hit)` is guaranteed non-null here since `hit` came from the `rows` array, which was filtered to elements with a resolvable path.)

- [ ] **Step 3: Type-check**

Run: `cd web && npx tsc --noEmit 2>&1 | tail -40`
Expected: no new errors.

- [ ] **Step 4: Manual smoke test (browser, touch emulation)**

Open the touch UI (device toolbar / touch emulation in devtools), tap a branch row while a clipboard is armed — confirm the target highlights `Into` in the mid-band and `After` near the row's edges; drag-reorder a leaf row into a single-line inline-table's mid-band and confirm it no longer offers "into" (previously it incorrectly did).

- [ ] **Step 5: Commit**

```bash
git add web/touch/app.ts
git commit -m "refactor(touch): tap/reorder into-eligibility via core pointer_slot, fixes threshold+Format drift (ADR 0004 §1)"
```

---

### Task 11: `web/touch/render.ts` + `web/touch/app.ts` — Into/After visual cue

**Files:**
- Modify: `web/touch/render.ts` (`rowHTML`/`treeHTML`)
- Modify: `web/touch/app.ts` (re-render hook for the `After` line)

**Interfaces:**
- Consumes: `SessionSnapshot.paste_slot`; reuses existing `.row.drop-into` (Into) and `.reorder-line` (After) CSS/DOM already used by `onReorderMove`.

- [ ] **Step 1: Implement**

In `web/touch/render.ts`, thread a `pasteInto: boolean` through `rowHTML`/`treeHTML`:

```ts
function rowHTML(r: ViewRow, idx: number, rows: ViewRow[], pasteInto: boolean): string {
  const branch = r.is_branch;
  const comment = isCommentRow(r);
  const pad = 10 + Math.max(0, r.depth - 1) * 18;
  const expanded = branch && isExpanded(rows, idx);
  const type = branch ? containerKind(r) : r.scalar_type ?? "string";
  const dataPath = esc(JSON.stringify(r.path));
  const cls =
    "row" +
    (branch ? " branch" : "") +
    (expanded ? " open" : "") +
    (r.selected ? " selected" : "") +
    (r.is_cursor ? " cursor" : "") +
    (r.read_only ? " readonly" : "") +
    (r.violations ? " schema-violation" : "") +
    (pasteInto ? " drop-into" : "");
```

(only the `cls` computation changes; the rest of `rowHTML`'s body is unchanged.)

```ts
export function treeHTML(snap: SessionSnapshot): string {
  const rows = snap.rows;
  const pasteIntoPath =
    snap.paste_slot && "Into" in snap.paste_slot ? JSON.stringify(snap.paste_slot.Into) : null;
  return (
    rows
      .map((r, idx) =>
        r.path.length === 0 ? "" : rowHTML(r, idx, rows, pasteIntoPath === JSON.stringify(r.path)),
      )
      .join("") + '<div class="reorder-line"></div>'
  );
}
```

In `web/touch/app.ts`, find the render function that calls `treeHTML(snap)` and rebuilds the tree DOM (search: `grep -n "treeHTML(" web/touch/app.ts`), and add — right after that DOM patch — positioning logic for the `After` slot, reusing `.reorder-line` (mirroring `onReorderMove`'s own positioning math):

```ts
  // The armed-paste `After` target reuses the same `.reorder-line` element
  // drag-reorder already positions (ADR 0004 §1); `Into` is baked into the
  // row's own class by `treeHTML` above.
  const reorderLine = treeEl.querySelector<HTMLElement>(".reorder-line");
  const slot = snap.paste_slot;
  if (reorderLine) {
    if (slot && "After" in slot) {
      const rowEl = treeEl.querySelector<HTMLElement>(
        `.row[data-path='${CSS.escape(JSON.stringify(slot.After))}']`,
      );
      if (rowEl) {
        const treeTop = treeEl.getBoundingClientRect().top;
        reorderLine.style.top = `${rowEl.getBoundingClientRect().bottom - treeTop}px`;
        reorderLine.style.display = "block";
      } else {
        reorderLine.style.display = "none";
      }
    } else if (!reordering) {
      reorderLine.style.display = "none";
    }
  }
```

(Guard on `!reordering` so a live drag's own positioning of `.reorder-line` in `onReorderMove` is never clobbered by a stale re-render mid-drag.)

- [ ] **Step 2: Type-check**

Run: `cd web && npx tsc --noEmit 2>&1 | tail -40`
Expected: no new errors.

- [ ] **Step 3: Manual smoke test (browser, touch emulation)**

Arm the clipboard, tap near the top/bottom edge of a leaf row — confirm the green insertion line appears at the right position; tap the mid-band of a branch row — confirm the row itself gets the `drop-into` highlight.

- [ ] **Step 4: Commit**

```bash
git add web/touch/render.ts web/touch/app.ts
git commit -m "feat(touch): Into/After visual cue for the armed paste target, reusing drag styling (ADR 0004 §1)"
```

---

## Phase 4: Drag copy-modifier

### Task 12: `web/dnd.ts` — ⌥/Ctrl held during drag-drop copies instead of moves

**Files:**
- Modify: `web/dnd.ts` (`dragover`, `drop` handlers)

**Interfaces:**
- Consumes: `Intent.MoveSelectionTo.cut` (Task 3).

- [ ] **Step 1: Implement**

In `web/dnd.ts`'s `dragover` handler, set the native `dropEffect` from the modifier state (standard HTML5 DnD API — the browser renders its own "+" copy cursor automatically, no custom CSS needed) — add right after the existing `if (ev.dataTransfer) ev.dataTransfer.dropEffect = "move";` line inside the eligibility branching (replace that one line with a modifier-aware version at the top of the handler, before the into/before/after branching):

```ts
  treeEl.addEventListener("dragover", (ev) => {
    if (!sources) return;
    ev.preventDefault(); // allow drop
    const copy = ev.altKey || ev.ctrlKey;
    if (ev.dataTransfer) ev.dataTransfer.dropEffect = copy ? "copy" : "move";
```

(remove the old, now-duplicate `if (ev.dataTransfer) ev.dataTransfer.dropEffect = "move";` line a few lines below, if the surrounding code still has it after Task 9's edit — confirm via `grep -n "dropEffect" web/dnd.ts` before editing, since Task 9 already touched this handler.)

In the `drop` handler, capture the modifier from the drop event itself (not the last `dragover`, since some browsers don't guarantee the final `dragover`'s modifier state matches `drop`'s) and thread `cut` through both `MoveSelectionTo` sends:

```ts
  treeEl.addEventListener("drop", (ev) => {
    if (!sources || !target) return endDrag();
    ev.preventDefault();
    const snap = getSnap();
    const src = sources;
    const tgt = target;
    const cut = !(ev.altKey || ev.ctrlKey);
    endDrag();
    if (!snap) return;
    if (tgt.mode === "into") {
      // Append as the last child (design pushes onto `children`).
      const idx = rowFor(snap, tgt.path)?.child_count ?? 0;
      send({ MoveSelectionTo: { sources: src, target: tgt.path, index: idx, cut } });
    } else {
      const sib = siblingIndex(snap.rows, tgt.path);
      send({
        MoveSelectionTo: {
          sources: src,
          target: parentOf(tgt.path),
          index: tgt.mode === "after" ? sib + 1 : sib,
          cut,
        },
      });
    }
  });
```

- [ ] **Step 2: Type-check**

Run: `cd web && npx tsc --noEmit 2>&1 | tail -40`
Expected: no new errors.

- [ ] **Step 3: Manual smoke test (browser)**

Drag a row while holding ⌥ (macOS) or Ctrl (Windows/Linux) — confirm the browser shows a copy cursor, the source row stays in place after drop, and the destination gains a copy; a plain drag (no modifier) still moves as before.

- [ ] **Step 4: Commit**

```bash
git add web/dnd.ts
git commit -m "feat(web): copy-modifier (⌥/Ctrl) drag-drop copies instead of moves (ADR 0004 §1)"
```

---

## Final Verification

### Task 13: Full workspace verification

**Files:** none (verification only).

- [ ] **Step 1: Full Rust workspace test suite**

Run: `cargo test 2>&1 | grep -E "test result|FAILED|error\[" | tail -60`
Expected: `test result: ok` for every crate, 0 failed.

- [ ] **Step 2: Full JS spec suite**

Run: `cd web && for f in *.spec.mjs; do echo "-- $f --"; node "$f" || exit 1; done`
Expected: every spec file exits 0.

- [ ] **Step 3: TypeScript type-check (web + touch)**

Run: `cd web && npx tsc --noEmit 2>&1 | tail -60`
Expected: no errors.

- [ ] **Step 4: Confirm the TUI is untouched**

Run: `git diff --stat main -- crates/confy-tui/`
Expected: empty output (no TUI source changes — ADR §2: "TUI | … | unchanged").

- [ ] **Step 5: Browser smoke test — the ADR's headline gap**

Launch the web dev server, load a sample TOML with a nested table, copy a leaf node, click the mid-band of a collapsed-then-expanded branch row — confirm the paste lands **inside** the branch (previously only `After(cursor)` was reachable from a click). Repeat on touch emulation with a tap.

- [ ] **Step 6: Update `CHANGELOG.md` `Unreleased` header timestamp if the repo convention requires one (check the file's existing entries for the pattern) and do a final review of the diff**

Run: `git diff --stat main`
Expected: matches the file list touched across Tasks 1-12, nothing unexpected.

- [ ] **Step 7: Commit (if any cleanup was needed) and stop — do not merge/push without explicit user review per this repo's `finishing-a-development-branch` conventions**
