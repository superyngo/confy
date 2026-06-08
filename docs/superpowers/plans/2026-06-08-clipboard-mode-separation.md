# Clipboard Mode Separation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Clearly separate "selection mode" from "clipboard mode" visually and behaviourally, and ensure paste failures never silently discard clipboard state.

**Architecture:** Four targeted patches to `src/tui/app.rs` and `src/tui/ui.rs`. No new types. The key insight is that `clipboard.is_some()` already distinguishes clipboard mode from plain selection mode — we just need to (1) enforce that distinction in key handlers, (2) propagate it through Esc, and (3) fix the non-collision paste-error path that currently drops the clipboard.

**Tech Stack:** Rust, ratatui — `cargo test`, `cargo clippy -- -D warnings`, `cargo fmt --check`

---

## Current State Summary

| Concern | Current behaviour | Target behaviour |
|---|---|---|
| Source node colour | `DarkGray` bg + `Cyan` fg | `Blue` bg (distinguishable from selection) |
| Selected node colour | `DarkGray` bg | `DarkGray` bg ✓ (unchanged) |
| `s` / Shift+Arrow in clipboard mode | still works | blocked — no-op |
| Esc from clipboard mode entered via selection | clears clipboard in one step | first press clears clipboard; second press clears selection |
| Esc from clipboard mode entered via cursor only | one step ✓ | one step ✓ (no change) |
| Non-collision paste error | clipboard **dropped** by `take()` | clipboard **preserved** |

---

## File Map

| File | Changes |
|---|---|
| `src/tui/app.rs` | `escape()`, `toggle_select()`, `extend_select_up()`, `extend_select_down()`, `do_paste()` |
| `src/tui/ui.rs` | `draw_tree()` — one style line for `in_clipboard_source` |

---

## Task 1 — Fix source-node colour (`Blue` not `DarkGray/Cyan`)

**Files:**
- Modify: `src/tui/ui.rs` — `draw_tree` styling block (~line 333)

### Context
`in_clipboard_source` rows currently render:
```rust
Style::default().bg(Color::DarkGray).fg(Color::Cyan)
```
`selection.contains(i)` rows render:
```rust
Style::default().bg(Color::DarkGray)
```
Both use `DarkGray` background — visually indistinguishable at a glance.
Target: source nodes get `Blue` background (same hue as the normal cursor) so they read as "these are the things that are captured", clearly different from the grey "these are selected" look.

- [ ] **Step 1: Change source-node style**

In `src/tui/ui.rs`, find the `in_clipboard_source` arm inside `draw_tree`:
```rust
} else if in_clipboard_source {
    // Copy/cut source: cyan tint
    Style::default().bg(Color::DarkGray).fg(Color::Cyan)
```
Replace with:
```rust
} else if in_clipboard_source {
    // Copy/cut source: blue bg to distinguish from grey multi-select
    Style::default().bg(Color::Blue).fg(Color::White)
```

- [ ] **Step 2: Verify build and tests pass**

```bash
cd /Volumes/Home/Users/wen/repos/confy
cargo test 2>&1 | tail -5
cargo clippy -- -D warnings 2>&1
cargo fmt --check 2>&1
```
Expected: all tests pass, no warnings, no format diff.

- [ ] **Step 3: Commit**

```bash
git add src/tui/ui.rs
git commit -m "style: use blue bg for clipboard source nodes (distinct from grey selection)"
```

---

## Task 2 — Block selection operations while clipboard is active

**Files:**
- Modify: `src/tui/app.rs` — `toggle_select()`, `extend_select_up()`, `extend_select_down()`

### Context
`toggle_select` (key `s`), `extend_select_up` (Shift+Up), `extend_select_down` (Shift+Down) are the three paths that mutate `self.selection`. While a clipboard is loaded (paste mode is active), these should be no-ops so the two modes stay cleanly separated.

Current `toggle_select` (line ~495):
```rust
pub fn toggle_select(&mut self) {
    self.selection.toggle(self.cursor);
}
```

Current `extend_select_up` (line ~501):
```rust
pub fn extend_select_up(&mut self) {
    if !self.last_action_was_shift_select {
        self.selection.begin_round(self.cursor);
    }
    if self.cursor > 0 {
        self.cursor -= 1;
        self.selection.extend_round_to(self.cursor);
    }
    self.last_action_was_shift_select = true;
}
```

Current `extend_select_down` (line ~513):
```rust
pub fn extend_select_down(&mut self) {
    if !self.last_action_was_shift_select {
        self.selection.begin_round(self.cursor);
    }
    if self.cursor + 1 < self.rows.len() {
        self.cursor += 1;
        self.selection.extend_round_to(self.cursor);
    }
    self.last_action_was_shift_select = true;
}
```

- [ ] **Step 1: Write a failing test**

Add to the `#[cfg(test)] mod tests` block at the bottom of `src/tui/app.rs`:

```rust
#[test]
fn selection_ops_are_blocked_while_clipboard_active() {
    let mut app = sample();
    // Move cursor to a leaf so we have something selectable.
    app.cursor = 1;
    // Load a clipboard (simulates copy).
    app.clipboard = Some(Clipboard {
        fragments: vec!["x = 1\n".into()],
        cut: false,
        sources: vec![vec![Seg::Key("a".into()), Seg::Key("x".into())]],
    });
    // toggle_select must be a no-op while clipboard is active.
    app.toggle_select();
    assert!(app.selection.is_empty(), "s should not select when clipboard active");
    // extend_select_down must not alter selection either.
    app.extend_select_down();
    assert!(app.selection.is_empty(), "Shift+Down should not select when clipboard active");
    // extend_select_up must not alter selection either.
    app.extend_select_up();
    assert!(app.selection.is_empty(), "Shift+Up should not select when clipboard active");
}
```

- [ ] **Step 2: Run the test to confirm it fails**

```bash
cargo test selection_ops_are_blocked_while_clipboard_active 2>&1
```
Expected: FAIL (toggle_select currently mutates selection unconditionally).

- [ ] **Step 3: Add clipboard guard to all three handlers**

In `src/tui/app.rs`, update the three functions:

```rust
pub fn toggle_select(&mut self) {
    if self.clipboard.is_some() {
        return; // clipboard mode: selection locked
    }
    self.selection.toggle(self.cursor);
}
```

```rust
pub fn extend_select_up(&mut self) {
    if self.clipboard.is_some() {
        return; // clipboard mode: use plain cursor movement instead
    }
    if !self.last_action_was_shift_select {
        self.selection.begin_round(self.cursor);
    }
    if self.cursor > 0 {
        self.cursor -= 1;
        self.selection.extend_round_to(self.cursor);
    }
    self.last_action_was_shift_select = true;
}
```

```rust
pub fn extend_select_down(&mut self) {
    if self.clipboard.is_some() {
        return; // clipboard mode: use plain cursor movement instead
    }
    if !self.last_action_was_shift_select {
        self.selection.begin_round(self.cursor);
    }
    if self.cursor + 1 < self.rows.len() {
        self.cursor += 1;
        self.selection.extend_round_to(self.cursor);
    }
    self.last_action_was_shift_select = true;
}
```

- [ ] **Step 4: Run the test to confirm it passes**

```bash
cargo test selection_ops_are_blocked_while_clipboard_active 2>&1
```
Expected: PASS.

- [ ] **Step 5: Run full test suite**

```bash
cargo test 2>&1 | tail -5
cargo clippy -- -D warnings 2>&1
cargo fmt --check 2>&1
```
Expected: all 167+ tests pass, no warnings, no format diff.

- [ ] **Step 6: Commit**

```bash
git add src/tui/app.rs
git commit -m "feat: block s/Shift+Arrow selection while clipboard is active"
```

---

## Task 3 — Two-step Esc when entering clipboard mode via selection

**Files:**
- Modify: `src/tui/app.rs` — `escape()` Normal branch

### Context
Current `escape()` Normal branch (lines 1334–1342):
```rust
Mode::Normal => {
    if !self.selection.is_empty() {
        self.selection.clear();
        self.last_action_was_shift_select = false;
        self.status = Some("selection cleared".into());
    } else if self.clipboard.is_some() {
        self.clipboard = None;
        self.status = None;
    }
}
```

The user experience goal:
- **Clipboard active + selection non-empty** (entered clipboard mode via `c`/`x` while selection was live):
  - First Esc → clear clipboard only (`clipboard = None`), keep selection, status hint "clipboard cleared"
  - Second Esc → clear selection (existing behaviour)
- **Clipboard active + selection empty** (entered clipboard mode from cursor only):
  - One Esc → clear clipboard (existing behaviour, no change)
- **No clipboard, selection non-empty** → clear selection (existing, no change)
- **Nothing active** → no-op (existing, no change)

The rationale: the user built a selection, then pressed `c`/`x` to enter clipboard mode. Esc should peel back the last operation first (clipboard), not jump past it to clear the selection.

- [ ] **Step 1: Write failing tests**

Add to `#[cfg(test)] mod tests` in `src/tui/app.rs`:

```rust
#[test]
fn esc_from_clipboard_with_selection_clears_clipboard_first() {
    let mut app = sample();
    app.cursor = 1;
    // Simulate: user selected row 1 then pressed 'c'
    app.selection.toggle(1);
    app.clipboard = Some(Clipboard {
        fragments: vec!["x = 1\n".into()],
        cut: false,
        sources: vec![vec![Seg::Key("a".into()), Seg::Key("x".into())]],
    });
    // First Esc: should clear clipboard, leave selection intact.
    app.escape();
    assert!(app.clipboard.is_none(), "first Esc must clear clipboard");
    assert!(!app.selection.is_empty(), "first Esc must leave selection intact");
    // Second Esc: should clear selection.
    app.escape();
    assert!(app.selection.is_empty(), "second Esc must clear selection");
}

#[test]
fn esc_from_clipboard_without_selection_clears_in_one_step() {
    let mut app = sample();
    // No selection — cursor-only clipboard.
    app.clipboard = Some(Clipboard {
        fragments: vec!["x = 1\n".into()],
        cut: false,
        sources: vec![vec![Seg::Key("a".into()), Seg::Key("x".into())]],
    });
    app.escape();
    assert!(app.clipboard.is_none(), "single Esc must clear clipboard");
    assert!(app.selection.is_empty(), "selection must stay empty");
}
```

- [ ] **Step 2: Run tests to confirm they fail**

```bash
cargo test esc_from_clipboard_with_selection 2>&1
cargo test esc_from_clipboard_without_selection 2>&1
```
Expected: both FAIL under current logic (first test: current code clears selection first, not clipboard).

- [ ] **Step 3: Rewrite the Normal branch of `escape()`**

In `src/tui/app.rs`, replace the `Mode::Normal` arm in `escape()`:

```rust
Mode::Normal => {
    if self.clipboard.is_some() {
        // Peel back clipboard mode first. If a selection was live when the
        // user pressed c/x, keep it — a second Esc will clear it below.
        self.clipboard = None;
        self.status = if !self.selection.is_empty() {
            Some("clipboard cleared".into())
        } else {
            None
        };
    } else if !self.selection.is_empty() {
        self.selection.clear();
        self.last_action_was_shift_select = false;
        self.status = Some("selection cleared".into());
    }
}
```

- [ ] **Step 4: Run new tests to confirm they pass**

```bash
cargo test esc_from_clipboard_with_selection 2>&1
cargo test esc_from_clipboard_without_selection 2>&1
```
Expected: both PASS.

- [ ] **Step 5: Run full suite**

```bash
cargo test 2>&1 | tail -5
cargo clippy -- -D warnings 2>&1
cargo fmt --check 2>&1
```
Expected: all tests pass, clean.

- [ ] **Step 6: Commit**

```bash
git add src/tui/app.rs
git commit -m "feat: two-step Esc for clipboard-over-selection; clipboard clears first"
```

---

## Task 4 — Preserve clipboard on non-collision paste failure

**Files:**
- Modify: `src/tui/app.rs` — `paste()` and `do_paste()`

### Context
`paste()` currently calls `self.clipboard.take()` unconditionally (line ~1251), which drops the clipboard before `do_paste` runs. If `do_paste` then hits a non-collision error (e.g., `MutateError::Fragment`, `MutateError::Unsupported`), the clipboard is gone and the user must re-copy.

The collision path already restores the clipboard (it re-sets `self.clipboard`). We need the same preservation for all other errors.

Current `paste()` relevant lines:
```rust
pub fn paste(&mut self) {
    let (fragments, is_cut, sources) = match self.clipboard.take() {  // <-- drops clipboard
        Some(cb) => (cb.fragments, cb.cut, cb.sources),
        None => {
            self.status = Some("clipboard empty".into());
            return;
        }
    };
    // ...calls do_paste(fragments, is_cut, sources, target, ...)
```

Current non-collision error arm in `do_paste` (line ~1300):
```rust
Err(e) => {
    self.status = Some(format!("paste error: {e}"));
    return;  // clipboard already gone from take()
}
```

Fix: pass the full `Clipboard` struct into `do_paste` instead of three separate fields, and let `do_paste` own the responsibility of re-installing it on any failure.

- [ ] **Step 1: Write a failing test**

Add to `#[cfg(test)] mod tests` in `src/tui/app.rs`:

```rust
#[test]
fn paste_error_preserves_clipboard() {
    // Build an app with a doc that has a table parent (not an array).
    let mut app = sample();
    // Craft a clipboard that will fail: trying to paste a bare value into a
    // table parent — that is a Fragment error from insert_fragment.
    // The cursor is on row 0 (root), which resolves to parent=[].
    // A fragment with no key ("42\n") fails into a Table parent.
    app.clipboard = Some(Clipboard {
        fragments: vec!["42\n".into()],
        cut: false,
        sources: vec![vec![Seg::Key("a".into())]],
    });
    app.paste();
    // The paste should fail (Fragment error) but clipboard must survive.
    assert!(
        app.clipboard.is_some(),
        "clipboard must be preserved after a paste error"
    );
    assert!(
        app.status.as_deref().map(|s| s.contains("paste error")).unwrap_or(false),
        "status must show the error"
    );
}
```

- [ ] **Step 2: Run to confirm it fails**

```bash
cargo test paste_error_preserves_clipboard 2>&1
```
Expected: FAIL (clipboard is `None` after the error under current code).

- [ ] **Step 3: Change `do_paste` signature to accept `Clipboard` and restore on any error**

Replace the existing `do_paste` signature and body in `src/tui/app.rs`.

**Old signature:**
```rust
pub(crate) fn do_paste(
    &mut self,
    fragments: Vec<String>,
    is_cut: bool,
    sources: Vec<Path>,
    target: Target,
    on_collision: OnCollision,
) {
```

**New signature and body:**
```rust
pub(crate) fn do_paste(
    &mut self,
    clipboard: Clipboard,
    target: Target,
    on_collision: OnCollision,
) {
    let Clipboard { fragments, cut: is_cut, sources } = clipboard;
    let doc = match self.doc.as_mut() {
        Some(d) => d,
        None => {
            // Restore clipboard so the user can try again.
            self.clipboard = Some(Clipboard { fragments, cut: is_cut, sources });
            return;
        }
    };
    for (i, frag) in fragments.iter().enumerate() {
        match doc.apply(Mutation::Insert {
            target: target.clone(),
            toml: frag.clone(),
            on_collision,
        }) {
            Ok(()) => {}
            Err(crate::model::document::MutateError::Collision(key)) => {
                // Put only the remaining unprocessed fragments back so retry
                // with Rename doesn't re-insert already-inserted fragments.
                self.clipboard = Some(Clipboard {
                    fragments: fragments[i..].to_vec(),
                    cut: is_cut,
                    sources,
                });
                self.status = Some(format!("collision on key '{key}' — o/r/c"));
                self.mode = Mode::Prompt(PromptKind::Collision { key });
                return;
            }
            Err(e) => {
                // Non-collision error: restore the full clipboard so the user
                // can navigate to a valid target and try again.
                self.clipboard = Some(Clipboard {
                    fragments: fragments[i..].to_vec(),
                    cut: is_cut,
                    sources,
                });
                self.status = Some(format!("paste error: {e}"));
                return;
            }
        }
    }
    // If cut, delete source nodes after successful paste.
    if is_cut {
        let mut sorted_sources = sources;
        sorted_sources.sort_by_key(|b| std::cmp::Reverse(b.len()));
        for src in &sorted_sources {
            if let Err(e) = doc.apply(Mutation::Delete { path: src.clone() }) {
                self.status = Some(format!("cut-delete error: {e}"));
                return;
            }
        }
    }
    self.on_mutation_success();
}
```

- [ ] **Step 4: Update `paste()` to pass a `Clipboard` struct instead of three fields**

Replace `paste()` in `src/tui/app.rs`:

```rust
/// `v` — paste clipboard fragments at insertion target.
/// On Collision: enters Mode::Prompt(Collision{key}).
/// If clipboard was cut, deletes sources after successful paste.
pub fn paste(&mut self) {
    let cb = match self.clipboard.take() {
        Some(cb) => cb,
        None => {
            self.status = Some("clipboard empty".into());
            return;
        }
    };
    let cursor_row = match self.rows.get(self.cursor) {
        Some(r) => r.clone(),
        None => {
            self.clipboard = Some(cb);
            return;
        }
    };
    let expanded = self.expanded.contains(&cursor_row.path);
    let sibling_index = self.true_sibling_index(&cursor_row.path);
    let target = crate::tui::insertion::resolve_target(&cursor_row, expanded, sibling_index);
    self.do_paste(cb, target, OnCollision::Cancel);
}
```

- [ ] **Step 5: Update the collision-prompt retry caller in `handle_prompt_key`**

Find the `PromptKind::Collision` arm in `handle_prompt_key` (it calls `do_paste` with three separate fields). Update it to pass a `Clipboard` struct.

Locate in `src/tui/app.rs` (search for `do_paste(fragments`). The call looks like:
```rust
self.do_paste(fragments, is_cut, sources, target, oc);
```
Replace with:
```rust
self.do_paste(
    Clipboard { fragments, cut: is_cut, sources },
    target,
    oc,
);
```

- [ ] **Step 6: Run the new test**

```bash
cargo test paste_error_preserves_clipboard 2>&1
```
Expected: PASS.

- [ ] **Step 7: Run full suite**

```bash
cargo test 2>&1 | tail -5
cargo clippy -- -D warnings 2>&1
cargo fmt --check 2>&1
```
Expected: all tests pass, clean.

- [ ] **Step 8: Commit**

```bash
git add src/tui/app.rs
git commit -m "fix: preserve clipboard on non-collision paste failure so user can retry"
```

---

## Task 5 — Remove per-row paste-validity dimming and red/green cursor split

**Files:**
- Modify: `src/tui/ui.rs` — `draw_tree` styling block
- Modify: `src/tui/app.rs` — remove `is_valid_paste_target` method

### Context
The current clipboard-mode row styling computes a per-row `valid_target` boolean and uses it to:
1. Show the cursor **green** when valid, **red** when invalid
2. **Dim** all non-source rows that are invalid paste targets

This turns out to add visual noise without practical benefit — the user has no way to know why a row is invalid without a textual error message, and the dim effect makes the tree harder to read. The better UX is to let the user try to paste anywhere and read the error in the status bar.

**Target behaviour:**
- Cursor in clipboard mode → always **green** (simple "you can paste here, try it" hint)
- No DIM on any rows
- `is_valid_paste_target` deleted (dead code once ui.rs stops calling it)
- `v` on an incompatible target still shows `"paste error: …"` in the status bar (existing behaviour via `do_paste`)

- [ ] **Step 1: Simplify `draw_tree` styling block**

In `src/tui/ui.rs`, replace the entire `valid_target` computation and the multi-branch cursor style with a simpler version:

**Find:**
```rust
            let is_cursor = i == app.cursor;
            let clipboard_active = app.clipboard.is_some();
            let in_clipboard_source = app
                .clipboard
                .as_ref()
                .is_some_and(|cb| cb.sources.contains(&row.path));
            let valid_target = clipboard_active && app.is_valid_paste_target(row);
            let style = if is_cursor && clipboard_active {
                if valid_target {
                    // Valid paste target: green
                    Style::default()
                        .bg(Color::Green)
                        .fg(Color::Black)
                        .add_modifier(Modifier::BOLD)
                } else {
                    // Invalid paste position: red so user knows to move cursor
                    Style::default()
                        .bg(Color::Red)
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD)
                }
            } else if is_cursor {
                Style::default()
                    .bg(Color::Blue)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else if in_clipboard_source {
                // Copy/cut source: cyan tint
                Style::default().bg(Color::DarkGray).fg(Color::Cyan)
            } else if clipboard_active && !valid_target {
                // Dim rows that cannot accept the paste — "dark room" effect
                Style::default().add_modifier(Modifier::DIM)
            } else if app.selection.contains(i) {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };
```

**Replace with:**
```rust
            let is_cursor = i == app.cursor;
            let clipboard_active = app.clipboard.is_some();
            let in_clipboard_source = app
                .clipboard
                .as_ref()
                .is_some_and(|cb| cb.sources.contains(&row.path));
            let style = if is_cursor && clipboard_active {
                // Clipboard active: green cursor signals paste-ready position.
                // Invalid targets show an error in the status bar on v.
                Style::default()
                    .bg(Color::Green)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD)
            } else if is_cursor {
                Style::default()
                    .bg(Color::Blue)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else if in_clipboard_source {
                // Copy/cut source: blue bg to distinguish from grey multi-select
                Style::default().bg(Color::Blue).fg(Color::White)
            } else if app.selection.contains(i) {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };
```

> Note: the `in_clipboard_source` style here already matches the Task 1 change — if Task 1 was already applied, that line is identical. If applying both tasks together, this block supersedes Task 1.

- [ ] **Step 2: Delete `is_valid_paste_target` from `app.rs`**

In `src/tui/app.rs`, find and delete the entire `is_valid_paste_target` method. It starts with:
```rust
    /// Return whether the cursor sitting on `row` would be a valid paste target for
```
and ends at its closing `}` (it is the last method before the final `}` of the `impl App` block).

Also delete `true_sibling_index` if it was only added to serve `is_valid_paste_target`.

> **Important:** `true_sibling_index` is **also** used by `paste()` and `add_node()` (Task 4 / previous session). Do **not** delete it. Only delete `is_valid_paste_target`.

- [ ] **Step 3: Build and confirm no dead-code warnings**

```bash
cargo build 2>&1
cargo clippy -- -D warnings 2>&1
```
Expected: clean — no unused method warnings (since `is_valid_paste_target` was `pub`, clippy may not warn, but the build must be clean).

- [ ] **Step 4: Run full suite**

```bash
cargo test 2>&1 | tail -5
cargo fmt --check 2>&1
```
Expected: all tests pass, no format diff.

- [ ] **Step 5: Commit**

```bash
git add src/tui/ui.rs src/tui/app.rs
git commit -m "refactor: remove paste-validity per-row dimming; cursor stays green, errors shown in status bar"
```

---

## Self-Review

**Spec coverage check:**

| Requirement | Task |
|---|---|
| Source nodes — Blue bg | Task 1 (superseded by Task 5 block) |
| Selected nodes — DarkGray bg (unchanged) | Task 1 (confirmed no change) |
| Block `s`/Shift+Arrow in clipboard mode | Task 2 |
| Esc two-step when clipboard entered via selection | Task 3 |
| Esc one-step when clipboard entered via cursor only | Task 3 |
| Paste failure preserves clipboard | Task 4 |
| Remove per-row validity dimming / red cursor | Task 5 |

**Placeholder scan:** No TBDs, no "similar to" references. All code blocks are complete.

**Type consistency:** `Clipboard` struct used consistently throughout (imported via `use crate::tui::state::Clipboard` which is already in scope in `app.rs`). `do_paste` new signature takes `Clipboard` — all callers updated in Tasks 4/5.
