# Row-State Visual Language (Phase 2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reverse the Enter/Space keybinding on both TUI and desktop so `Space` always
`ToggleExpand`s and `Enter` always `ToggleDetail`s (`i` stays the unchanged alt detail
binding on both hosts), per ADR 0005 §4 / `ROW_STATE_MODEL.md` §4. Touch has no physical
Enter/Space tree binding and needs no change (ROW_STATE_MODEL.md §4 note, confirmed:
`web/touch/app.ts`'s only keydown handler is the URL-open sheet input, unrelated).

**Architecture:** Two independent per-surface changes, no shared runtime code. TUI:
`crates/confy-tui/src/tui/keys.rs`'s `map_key` — a single match arm currently binds both
keys to `KeyAction::ToggleExpand`; splitting it is sufficient because `KeyAction::Info`
(bound to `i`) already routes to `app.toggle_detail()` — no new enum variant, no `mod.rs`
dispatch change. Desktop: `web/key-intent.ts`'s `resolveKeyIntent` — swap which of the two
existing branches (`native`/`toggle-branches` vs `intent`/`ToggleDetail`) each key hits;
`toggleSelectedBranches()` and `Intent::ToggleDetail` both already exist and are unchanged.
Both sides keep their `preventDefault` semantics tied to the *action*, not the key: Space's
`preventDefault: true` (native-scroll suppression) now travels with the native
`toggle-branches` branch instead of the `ToggleDetail` branch.

**Tech Stack:** Rust (TUI), TypeScript + esbuild, no framework (desktop). Both sides
already have a keys-to-intent test file to extend (`crates/confy-tui/src/tui/keys.rs`'s
inline `mod tests`, `web/key-intent.spec.mjs`).

**Spec:** `ROW_STATE_MODEL.md` §4 (keybinding table), `docs/adr/0005-row-cursor-selection-clipboard-state-model.md`.

## Global Constraints

- `i` is untouched on both hosts — it stays the alt `ToggleDetail` binding
  (`tui/keys.rs:63`, `key-intent.ts:179`). Do not rename, move, or duplicate its match arm.
- No `Intent`/wire-format/snapshot changes; no `mod.rs` dispatch-table change on the TUI
  side (`KeyAction::Info` already routes to `toggle_detail()`, `KeyAction::ToggleExpand`'s
  existing paste-mode/branch/leaf handling is unchanged and untouched by this phase).
- Touch (`web/touch/`) is explicitly out of scope — do not add or change anything there.
- Every doc/help-copy surface enumerated below carries the *current* (soon-to-be-wrong)
  binding verbatim today; this phase's docs-sync is not optional busywork, it is the
  actual behavior change becoming user-visible truth. Do it in the same task as the code
  change, per ADR 0004's own lesson about docstring drift (already cited by
  `ROW_STATE_MODEL.md` §8's "Docs sync" note).
- Preserve each file's existing fixed-column text alignment when editing help/cheatsheet
  copy — exact target strings are given below precisely so no realignment guesswork is
  needed.

---

### Task 1: TUI — reverse `Enter`/`Space`, sync TUI-side docs

**Files:**
- Modify: `crates/confy-tui/src/tui/keys.rs:56` (the shared match arm), append test to the
  existing `mod tests` block (same file).
- Modify: `crates/confy-tui/src/tui/mod.rs:405-407, 421-423` (two stale "Enter/Space"
  comments — Space-only now).
- Modify: `i18n/en.json:67,68,69` (`tui.help.toml`/`.json`/`.yaml`, one edit per string, same
  substitution in each).
- Modify: `i18n/zh-TW.json:68,69,70` (same three strings, zh-TW translation).
- Modify: `README.md:118` (keybinding table — split into two rows).

**Interfaces:**
- Consumes/produces nothing new — `KeyAction::Info` and `KeyAction::ToggleExpand` both
  already exist and are both already dispatched in `mod.rs`. Task 2 is fully independent
  (disjoint files, disjoint language/runtime).

- [ ] **Step 1: Write the failing test**

Append inside the existing `#[cfg(test)] mod tests { ... }` block in
`crates/confy-tui/src/tui/keys.rs` (after `help_text_is_translated_for_zh_tw`, before the
module's closing `}`):

```rust
#[test]
fn enter_opens_detail_space_toggles_expand() {
    use crossterm::event::{KeyEvent, KeyModifiers};
    let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    assert!(
        matches!(map_key(enter), KeyAction::Info),
        "Enter must route to the same detail-toggle action as `i` (ADR 0005 §4)"
    );
    let space = KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE);
    assert!(
        matches!(map_key(space), KeyAction::ToggleExpand),
        "Space must keep ToggleExpand — only Enter's binding reverses"
    );
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p confy-tui --lib tui::keys::tests::enter_opens_detail_space_toggles_expand -- --nocapture`

Expected: FAIL on the first assertion — `map_key(enter)` currently returns
`KeyAction::ToggleExpand`, not `KeyAction::Info`.

- [ ] **Step 3: Implement**

Replace `crates/confy-tui/src/tui/keys.rs:56`:

```rust
        (KeyCode::Char(' '), _) => KeyAction::ToggleExpand,
        (KeyCode::Enter, _) => KeyAction::Info,
```

(Keep this pair adjacent to where the single combined arm was — do not reorder relative to
neighboring arms.)

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p confy-tui --lib tui::keys::tests::enter_opens_detail_space_toggles_expand -- --nocapture`

Expected: PASS.

- [ ] **Step 5: Fix the two stale dispatch comments**

In `crates/confy-tui/src/tui/mod.rs`, re-read the file first (line numbers may have shifted
from the citations below since Phase 1 landed):

Replace the comment at (originally) `mod.rs:405-407`:
```rust
                        // Paste mode: only the `Into` (on-branch) slot toggles the
                        // branch; the green-line `After` slot is about the gap, not
                        // the branch, so Space is a no-op there.
```

Replace the comment at (originally) `mod.rs:421-423`:
```rust
                        // Space: branch toggles expand, leaf opens detail — the
                        // actual decision lives once in `Session::apply`; only
                        // rebuild when it does (opening detail changes no rows).
```

(Both comments said "Enter/Space" — Enter no longer reaches this `KeyAction::ToggleExpand`
arm at all, so both become Space-only. No other line in this match arm changes — `mod.rs`'s
`keys::KeyAction::Info => app.toggle_detail()` arm already exists and needs no edit.)

- [ ] **Step 6: Sync the TUI help-overlay catalog (`i18n/en.json`)**

In `i18n/en.json`, within each of the three strings `tui.help.toml`, `tui.help.json`,
`tui.help.yaml` (lines 67-69), make the identical substitution twice per string:

Replace `" Enter/Space  Expand branch or open leaf detail\n"` with
`" Space        Expand branch or open leaf detail\n"` (5-char key + 8 spaces = same
14-column field the original 11-char key + 2 spaces produced — verify by counting, don't
eyeball).

Replace `" i            Detail/info popup (any node)\n"` with
`" Enter / i    Detail/info popup (any node)\n"` (9-char key + 4 spaces = same 14-column
field).

- [ ] **Step 7: Sync the TUI help-overlay catalog (`i18n/zh-TW.json`)**

In `i18n/zh-TW.json`, within each of the three strings `tui.help.toml`, `tui.help.json`,
`tui.help.yaml` (lines 68-70), make the identical substitution twice per string:

Replace `" Enter/Space  展開分支或開啟葉節點詳細資訊\n"` with
`" Space        展開分支或開啟葉節點詳細資訊\n"`.

Replace `" i            詳細資訊彈出視窗（任何節點）\n"` with
`" Enter / i    詳細資訊彈出視窗（任何節點）\n"`.

- [ ] **Step 8: Run the i18n parity test (catches any accidental key drift between en/zh-TW)**

Run: `cargo test -p confy-core --lib session::i18n`

Expected: PASS — key parity + placeholder counts between `en.json`/`zh-TW.json` are
unaffected by re-wording values, only by adding/removing keys (which this step does not
do).

- [ ] **Step 9: Update the README keybinding table**

Replace `README.md:118`:

```markdown
| `Space` | Expand/collapse branch, or open leaf detail if it's a leaf |
| `Enter` | Toggle the detail/info popup for the cursor row (same as `i`) |
```

(Splits the old combined row into two; the existing `i` row directly below,
`README.md:119`, is unchanged.)

- [ ] **Step 10: Run the full TUI test suite**

Run: `cargo test -p confy-tui`

Expected: PASS, including the new test and every pre-existing one (no other test asserts
on `map_key`, per the pre-implementation scan).

- [ ] **Step 11: Commit**

```bash
git add crates/confy-tui/src/tui/keys.rs crates/confy-tui/src/tui/mod.rs i18n/en.json i18n/zh-TW.json README.md
git commit -m "fix(tui): reverse Enter/Space bindings — Space expands, Enter opens detail (ADR 0005 §4)"
```

---

### Task 2: Desktop — reverse `Enter`/`Space`, sync desktop-side docs

**Files:**
- Modify: `web/key-intent.ts:155-156` (the two case arms).
- Modify: `web/key-intent.spec.mjs:252-255` (flip the existing Enter assertion, add a new
  Space assertion next to it).
- Modify: `web/help-content.ts:8, 17, 30, 39, 53, 62, 77, 86` (4 blocks × 2 lines each).
- Modify: `WEBUI.md:139-143, 205-207, 422`.

**Interfaces:**
- Consumes/produces nothing new — `"native"/"toggle-branches"` (→ `toggleSelectedBranches()`)
  and `"intent"/"ToggleDetail"` both already exist in `key-intent.ts`/`ui.ts` and are
  unchanged by this task. `i` (`key-intent.ts:179`) is untouched. Task 1 is fully
  independent (disjoint files, disjoint language/runtime).

- [ ] **Step 1: Write the failing test**

In `web/key-intent.spec.mjs`, replace the block at (originally) lines 252-255:

```js
{
  const r = resolve(normalMode, "Enter");
  check("Enter -> intent ToggleDetail, no preventDefault", r?.kind === "intent" && r.intent === "ToggleDetail" && r.preventDefault === false, JSON.stringify(r));
}
{
  const r = resolve(normalMode, " ");
  check('"Space" -> native toggle-branches, preventDefault', r?.kind === "native" && r.action === "toggle-branches" && r.preventDefault === true, JSON.stringify(r));
}
```

(The existing `resolve(typeFilterMode, " ")` case elsewhere in this file, originally at
`:167-168`, is mode-scoped to `TypeFilter` and is untouched — do not confuse it with this
new normal-mode Space case.)

- [ ] **Step 2: Run the spec to verify it fails**

Run: `node web/key-intent.spec.mjs`

Expected: the two new/changed checks report `✗` — `resolve(normalMode, "Enter")` currently
returns `{ kind: "native", action: "toggle-branches" }`, and `resolve(normalMode, " ")`
currently returns `{ kind: "intent", intent: "ToggleDetail" }` — both the inverse of the
new assertions. Overall exit code non-zero.

- [ ] **Step 3: Implement**

Replace `web/key-intent.ts:155-156`:

```ts
    case "Enter": return { kind: "intent", intent: "ToggleDetail", preventDefault: false };
    case " ": return { kind: "native", action: "toggle-branches", preventDefault: preSwitchPD };
```

(`preSwitchPD` — defined just above at what is currently line 149 — already includes `" "`
in its key list; keeping it on the `" "` arm preserves the existing native-scroll
suppression. Enter's `preventDefault: false` is unchanged in value, just now attached to
the `ToggleDetail` arm instead of the `toggle-branches` arm.)

- [ ] **Step 4: Run the spec to verify it passes**

Run: `node web/key-intent.spec.mjs`

Expected: all checks `✓`, exit code 0.

- [ ] **Step 5: Sync the desktop help overlay (`web/help-content.ts`)**

Four blocks, each needs its `Enter/Space` line changed to `Space`-only and its `i` line
combined with `Enter`. Column field width is 15 (verify by counting spaces in the original
before assuming — do not eyeball).

`HELP_TEXT` (EN, desktop) — replace line 8:
`Enter/Space    toggle branch / edit leaf / activate` →
`Space          toggle branch / edit leaf / activate`
and line 17:
`i              detail popup · ? this help · Ctrl-s save · Ctrl-o open` →
`Enter / i      detail popup · ? this help · Ctrl-s save · Ctrl-o open`

`HELP_TEXT_ZH_TW` — replace line 30:
`Enter/Space    展開分支／編輯葉節點／啟用` →
`Space          展開分支／編輯葉節點／啟用`
and line 39:
`i              詳細資訊彈出視窗 · ? 本說明 · Ctrl-s 儲存 · Ctrl-o 開啟` →
`Enter / i      詳細資訊彈出視窗 · ? 本說明 · Ctrl-s 儲存 · Ctrl-o 開啟`

`HELP_TEXT_VSCODE` (EN, VS Code — no Ctrl-s/Ctrl-o line, they're VS Code's own) — replace
line 53:
`Enter/Space    toggle branch / edit leaf / activate` →
`Space          toggle branch / edit leaf / activate`
and line 62:
`i              detail popup · ? this help` →
`Enter / i      detail popup · ? this help`

`HELP_TEXT_VSCODE_ZH_TW` — replace line 77:
`Enter/Space    展開分支／編輯葉節點／啟用` →
`Space          展開分支／編輯葉節點／啟用`
and line 86:
`i              詳細資訊彈出視窗 · ? 本說明` →
`Enter / i      詳細資訊彈出視窗 · ? 本說明`

- [ ] **Step 6: Sync `WEBUI.md` prose**

Replace the sentence at (originally) `WEBUI.md:139-141`:

```markdown
  With a **multi-selection**, `Space` toggles every selected branch independently (cursor-walks
  the selected branch rows dispatching `ToggleExpand`, then restores the selection); a single
  selection keeps the plain cursor toggle.
```

(Only the key name changes, `Enter` → `Space` — `toggleSelectedBranches()`'s behavior itself
is untouched by this phase.)

Leave `WEBUI.md:142-143` (`Navigation keys (←→↑↓, Home/End, Space) preventDefault ...`) as
written — Space's `preventDefault` behavior is unchanged, only which action it triggers
changed.

Replace the parenthetical at (originally) `WEBUI.md:206`:

```markdown
  (`SetCursor` + `ToggleDetail`); it no longer toggles branch-expand/boolean-value (expand stays
  on the caret + Space).
```

Replace the parenthetical at (originally) `WEBUI.md:422`:

```markdown
desktop side the detail `<aside>` (toggled with `i`/Enter) now renders this panel
```

- [ ] **Step 7: Run the full web spec suite**

Run: `node web/run-tests.mjs`

Expected: PASS, all spec files including the modified `key-intent.spec.mjs`.

- [ ] **Step 8: Commit**

```bash
git add web/key-intent.ts web/key-intent.spec.mjs web/help-content.ts WEBUI.md
git commit -m "fix(web): reverse Enter/Space bindings — Space expands, Enter opens detail (ADR 0005 §4)"
```

---

## Final integration check

- [ ] Run `cargo test -p confy-tui -p confy-core`, `node web/run-tests.mjs` one more time
  together — both must be green with both tasks' changes present simultaneously (they touch
  disjoint files, so this is a formality, not expected to surface new conflicts).
- [ ] Re-read `ROW_STATE_MODEL.md` §8 Phase 2 checklist items and tick them off in that file
  to match this plan's completion.
