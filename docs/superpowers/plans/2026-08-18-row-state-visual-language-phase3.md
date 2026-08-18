# Row-State Visual Language (Phase 3) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement Phase 3 of ADR 0005 (`ROW_STATE_MODEL.md` §5): cut/copy modal lock across all three surfaces. When `clipboard.is_some()` (or `clipboard_count > 0`), all functions except `ToggleExpand`, navigation, paste commit (`v`/`p`), escape (`Esc`), and quit (`q`) are disabled, and attempting a disabled affordance displays a transient toast/status message.

**Architecture:**
- **Single Source of Truth:** `confy-core` `Session` defines the modal lock invariant: when `self.clipboard.is_some()`, all mutating or mode-entering operations set `self.status = Some(tr(self.lang, "core.clipboard.action-locked").to_string())` and return early without mutating the document or entering another modal mode.
- **i18n:** New string `core.clipboard.action-locked` added to both `i18n/en.json` and `i18n/zh-TW.json`.
- **TUI:** `tui/mod.rs` guards TUI-specific modal entries (`EditExternal`, `LangPicker`) with the same status message.
- **Desktop:** `web/dnd.ts` disables reorder drag during paste mode; `web/ui.ts` gates context menus, kind badge popovers, and toolbar buttons with the status banner.
- **Touch:** `web/touch/app.ts` gates reorder-grip drag, swipe-to-delete, double-tap detail sheet, and toolbar buttons, emitting `toast(tr(...))`.

---

### Task 1: Core Modal Lock & i18n (`crates/confy-core`, `i18n/`)

**Files:**
- `i18n/en.json`
- `i18n/zh-TW.json`
- `crates/confy-core/src/session/session.rs`
- `crates/confy-core/src/session/clipboard.rs`
- `crates/confy-core/src/session/inline_edit.rs`
- `crates/confy-core/src/session/undo_redo.rs`
- `crates/confy-core/src/session/search.rs`
- `crates/confy-core/src/session/type_filter.rs`
- `crates/confy-core/tests/modal_lock.rs` (new test file)

- [ ] **Step 1: Add i18n key**
  In `i18n/en.json` and `i18n/zh-TW.json`, add:
  - `en.json`: `"core.clipboard.action-locked": "action disabled while clipboard is armed — paste (v) or discard (Esc) first"`
  - `zh-TW.json`: `"core.clipboard.action-locked": "剪貼簿使用中，此操作已停用 —— 請先貼上（v）或捨棄（Esc）"`

- [ ] **Step 2: Write failing test in `crates/confy-core/tests/modal_lock.rs`**
  Create `modal_lock.rs` testing that when `session.copy_selected()` or `session.cut_selected()` is called (arming the clipboard):
  1. `session.add_node()`, `session.add_child()`, `session.add_sibling()` do not add nodes and set `status` to `core.clipboard.action-locked`.
  2. `session.delete_selected()` does not delete and sets `status`.
  3. `session.nudge(1)` does not mutate and sets `status`.
  4. `session.remark()` does not mutate and sets `status`.
  5. `session.begin_inline_edit()`, `session.begin_external_edit()`, `session.begin_inline_rename()` do not enter `Mode::Edit` and set `status`.
  6. `session.open_kind_switch()` does not enter `Mode::KindSwitch` and sets `status`.
  7. `session.open_convert()` does not enter `Mode::Convert` and sets `status`.
  8. `session.enter_filter()`, `session.enter_type_filter()` do not enter search/filter modes and set `status`.
  9. `session.undo()`, `session.redo()` do not perform undo/redo and set `status`.
  10. `session.toggle_detail()`, `session.enter_help()` do not enter detail/help modes and set `status`.
  11. `session.move_selection_to(...)` (drag move) does not mutate and sets `status`.
  12. `session.toggle_expand()` on a branch STILL succeeds (allowed invariant).

- [ ] **Step 3: Run test to confirm failure**
  Run `cargo test --test modal_lock` and observe failures.

- [ ] **Step 4: Implement guards in `crates/confy-core`**
  Add helper on `Session`:
  ```rust
  fn guard_clipboard_locked(&mut self) -> bool {
      if self.clipboard.is_some() {
          self.status = Some(tr(self.lang, "core.clipboard.action-locked").to_string());
          true
      } else {
          false
      }
  }
  ```
  Guard all the methods listed above.

- [ ] **Step 5: Run tests to confirm PASS**
  Run `cargo test -p confy-core` and verify all tests pass.

- [ ] **Step 6: Commit Task 1**
  `git commit -m "feat(core): lock mutations and modal entries while clipboard is armed (ADR 0005 §5)"`

---

### Task 2: TUI Modal Lock (`crates/confy-tui`)

**Files:**
- `crates/confy-tui/src/tui/mod.rs`
- `crates/confy-tui/src/tui/keys.rs`

- [ ] **Step 1: Write TUI test**
  In `crates/confy-tui/src/tui/keys.rs` (or a dedicated integration test), verify key actions during armed clipboard.

- [ ] **Step 2: Add TUI-specific guards in `crates/confy-tui/src/tui/mod.rs`**
  In `run_event_loop`:
  - `KeyAction::EditExternal`: if `app.session.clipboard.is_some()`, set `app.session.status = Some(tr(app.session.lang, "core.clipboard.action-locked").to_string())` and continue without launching `$EDITOR`.
  - `KeyAction::LangPicker`: if `app.session.clipboard.is_some()`, set status and continue.

- [ ] **Step 3: Run TUI tests**
  Run `cargo test -p confy-tui` and verify all pass.

- [ ] **Step 4: Commit Task 2**
  `git commit -m "feat(tui): guard external editor and language picker while clipboard is armed (ADR 0005 §5)"`

---

### Task 3: Desktop UI Modal Lock (`web/`)

**Files:**
- `web/dnd.ts`
- `web/ui.ts`
- `web/modal-lock.spec.mjs` (new test file)

- [ ] **Step 1: Write unit test in `web/modal-lock.spec.mjs`**
  Test that during `paste-mode`:
  1. `dnd.ts` `onDragStart` is prevented.
  2. Right-click context menu and KIND badge clicks are suppressed.
  3. Toolbar buttons (Undo, Redo, AttachSchema) do not trigger actions when in paste mode.

- [ ] **Step 2: Implement desktop UI guards**
  - In `web/dnd.ts`: in `onDragStart`, check `if (document.body.classList.contains("paste-mode")) { e.preventDefault(); return; }`.
  - In `web/ui.ts`:
    - Context menu listener (right click on row): if `snap.clipboard_count > 0`, `e.preventDefault()`, do not open context menu; set status banner to `tr("core.clipboard.action-locked")`.
    - Kind badge click in `onTreeClick`: if `snap.clipboard_count > 0`, do not open kind menu.
    - Toolbar buttons in `bindToolbar()`: if in paste-mode, show status message.

- [ ] **Step 3: Run web tests**
  Run `node web/run-tests.mjs` and verify all checks pass.

- [ ] **Step 4: Commit Task 3**
  `git commit -m "feat(web): disable reorder drag, context menu, and kind switch in paste mode (ADR 0005 §5)"`

---

### Task 4: Touch UI Modal Lock (`web/touch/`)

**Files:**
- `web/touch/app.ts`
- `web/touch-modal-lock.spec.mjs` (or add to `touch-pointer-slot.spec.mjs`)

- [ ] **Step 1: Write touch modal lock unit test**
  Assert that when `snap.clipboard_count > 0`:
  1. Grip drag does not start reorder.
  2. Left swipe does not reveal delete button.
  3. Double tap does not open detail sheet.
  4. Toolbar buttons emit toast.

- [ ] **Step 2: Implement touch UI guards in `web/touch/app.ts`**
  - `onReorderStart` (in `installTreeGestures`): if `snap.clipboard_count > 0`, do not start reorder drag; call `toast(tr("core.clipboard.action-locked"))`.
  - Swipe-to-delete in `installTreeGestures`: if `snap.clipboard_count > 0`, do not reveal `.row-del`.
  - `openPanel` (double-tap): if `snap.clipboard_count > 0`, do not open detail sheet; call `toast(tr("core.clipboard.action-locked"))`.
  - Toolbar buttons: if `snap.clipboard_count > 0`, intercept and `toast(tr("core.clipboard.action-locked"))`.

- [ ] **Step 3: Run touch tests**
  Run `node web/run-tests.mjs` and verify all touch checks pass.

- [ ] **Step 4: Commit Task 4**
  `git commit -m "feat(touch): disable grip reorder, swipe delete, and detail sheet in paste mode (ADR 0005 §5)"`

---

### Task 5: Docs Sync, Full Suite Verification & Merging

**Files:**
- `ROW_STATE_MODEL.md`
- `CHANGELOG.md`

- [ ] **Step 1: Tick off Phase 3 in `ROW_STATE_MODEL.md`**
  Tick the Phase 3 checkboxes in §8.

- [ ] **Step 2: Add Unreleased entry in `CHANGELOG.md`**
  Record the modal lock feature and behavioral details under `### Fixed` / `### Added`.

- [ ] **Step 3: Run full Rust & Web test suites**
  `cargo test -p confy-core -p confy-tui` and `cd web && node run-tests.mjs`.

- [ ] **Step 4: Commit Task 5**
  `git commit -m "docs(row-state): tick Phase 3 checklist and add changelog entry"`
