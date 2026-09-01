# Nudge redesign: remove boolean nudge, gate numeric wheel/gesture nudge to inline-edit mode

## Context

Two design reversals to `confy`'s value-nudge feature, approved with mechanism A (stateless core query, host writes the edit-buffer string, single commit on blur/Enter — no per-tick document mutation):

1. Remove every boolean nudge affordance (arrow-key `←/→`, `+/-`, mouse wheel) on every platform (TUI, web desktop, web touch). Bool editing remains available exclusively through the existing true/false picker (`Mode::SchemaEnum`), which every platform already renders.
2. Numeric (Integer/Float) wheel-nudge (web desktop tree + shared panel) and touch swipe-nudge (shared panel) must stop firing merely on pointer-hover/idle-drag. They must fire only once the value has entered inline-edit mode (the field is focused / the editor is open), and once armed, capture *all* wheel ticks / horizontal swipes anywhere (not just while the pointer sits over the input) until the edit ends (blur, Enter, or Escape).

End state: `nudge_scalar` never touches Bool; the web tree's hover-wheel-toggles-bool path is gone; wheel/swipe nudging on numbers only starts after the value field is focused, writes directly into the DOM input via a new stateless core query (`nudge_repr`), and commits once via the existing `CommitEdit` path — never dispatching `Intent::Nudge` from wheel/swipe.

## Approach

### 1. Core: drop Bool from `nudge_scalar`

`crates/confy-core/src/session/schema_hint.rs`, `nudge_scalar` (line 7 `pub(crate) fn nudge_scalar`): delete the `ScalarType::Bool => match s { "true" => Some("false".into()), "false" => Some("true".into()) }` arm (lines 10–13). Let it fall through to the existing wildcard/default arm that already returns `None` for non-nudgeable types (String/Datetime) — read the current full match before editing to confirm the exact wildcard shape and add `ScalarType::Bool` to it, or leave the wildcard `_ => None` (whichever the existing match uses) to also cover `Bool`. Do not touch the Integer/Float arms.

This single change silently no-ops `Intent::Nudge` on a Bool node for every intent-based caller: TUI `←/→` (`crates/confy-tui/src/tui/mod.rs:653-654` → `app.nudge`), TUI/web/FFI `Intent::Nudge` dispatch (`dispatch.rs:246`), web keyboard `+/-`/`←/→` (`key-intent.ts:240-241`), and the new `nudge_repr` query added in step 3 (which reuses `nudge_scalar`).

Update the existing unit test `crates/confy-core/src/session/session.rs:2158-2191` (`nudge_scalar_steps_each_type_preserving_format`): replace the assertion at lines 2182-2185 —
```rust
        assert_eq!(
            nudge_scalar(ScalarType::Bool, Format::Plain, "true", 1).as_deref(),
            Some("false")
        );
```
with an assertion that it returns `None`, grouped with the existing "strings / datetimes are not nudgeable" comment/assertion immediately below (lines 2186-2190):
```rust
        // bool / strings / datetimes are not nudgeable
        assert_eq!(nudge_scalar(ScalarType::Bool, Format::Plain, "true", 1), None);
        assert_eq!(
            nudge_scalar(ScalarType::String, Format::BasicString, "\"hi\"", 1),
            None
        );
```

### 2. Web desktop tree: remove the hover-wheel bool toggle and int/float wheel-on-hover

`web/ui.ts`:
- Delete `toggleBool()` (lines 1358-1368, including its leading comment) — no longer called anywhere after this step.
- Replace `onTreeWheel` (lines 1370-1390) entirely: the tree no longer nudges on wheel-over-value at all (numeric wheel-nudge moves to the focused-input-only path in step 4, which is armed by the input itself, not the tree's delegated wheel listener). Delete `onTreeWheel` and its registration `tree.addEventListener("wheel", onTreeWheel, { passive: false })` (line 1903), so wheel over the tree always falls through to native page scroll.
- Grep to confirm no other reference to `onTreeWheel`/`toggleBool` remains: `grep -n "onTreeWheel\|toggleBool" web/ui.ts` must return nothing after the edit.

### 3. Core: add a stateless `nudge_repr` query (no mutation, no mode change)

`crates/confy-core/src/session/inline_edit.rs`, add a new `pub fn` immediately after `schema_clamp_nudge` (after line 818, before `pub fn nudge` at line 820):

```rust
/// Stateless preview of nudging `text` — the host's *current edit-buffer*
/// string, which may differ from the committed node value — by `delta`
/// steps, without mutating the document or session mode. `None` when
/// `path` isn't a nudgeable scalar (bool/string/datetime — see
/// `nudge_scalar`) or `text` doesn't parse for its type. Read-only sibling
/// of `nudge()`: same `nudge_scalar` + `schema_clamp_nudge` pipeline, but
/// the caller decides whether/when to commit the result. Used by the Web/
/// touch wheel and swipe nudge while inline-editing (WEBUI.md), which
/// writes the result straight into the focused `<input>` and commits once
/// via the normal `CommitEdit` path rather than dispatching per tick.
pub fn nudge_repr(&self, path: &crate::model::node::Path, text: &str, delta: i64) -> Option<String> {
    let node = self.tree.node_at(path)?;
    let st = match node.kind {
        NodeKind::Scalar(st) => st,
        _ => return None,
    };
    let new_repr = nudge_scalar(st, node.format, text, delta)?;
    self.schema_clamp_nudge(path, &new_repr)
}
```

Confirm `NodeKind` and `nudge_scalar` are already imported in this file (they are — used by `nudge()` at lines 847-857) before writing; do not add redundant imports.

Add a unit test in `crates/confy-core/src/session/session.rs`'s existing `#[cfg(test)] mod tests` (same block as `nudge_scalar_steps_each_type_preserving_format`, `schema_clamp_nudge_snaps_to_multiple_of_and_clamps_to_bounds`), named `nudge_repr_previews_without_mutating`:
```rust
#[test]
fn nudge_repr_previews_without_mutating() {
    let mut s = session_from("port = 8080\n", DocFormat::Toml);
    s.dispatch(Intent::CursorDown);
    let path = s.cursor_row().unwrap().path;
    let preview = s.nudge_repr(&path, "8080", 1);
    assert_eq!(preview.as_deref(), Some("8081"));
    // The document itself is untouched — no Replace applied.
    assert!(s.doc.as_ref().unwrap().serialize().contains("port = 8080"));
    assert!(!s.is_dirty());
    // Bool path: excluded (mirrors nudge_scalar).
    let mut sb = session_from("flag = true\n", DocFormat::Toml);
    sb.dispatch(Intent::CursorDown);
    let bpath = sb.cursor_row().unwrap().path;
    assert_eq!(sb.nudge_repr(&bpath, "true", 1), None);
}
```
Before writing this test, read the existing `session_from`/`dispatch_nudge_clamps_to_schema_maximum` helpers (`crates/confy-core/tests/schema_headless.rs:794-812` and any in-crate `session_from` helper already used by the Bounded test at `session.rs:2217-2274`) to match the exact helper name/signature already in scope in that test module — reuse it verbatim rather than reinventing a session constructor.

### 4. FFI: expose `nudge_repr`

`crates/confy-ffi/src/lib.rs`: add a new `#[wasm_bindgen]` method on `ConfySession`, placed immediately after `schema_hint` (after line 141, mirroring its exact shape at lines 137-141):
```rust
/// Stateless nudge preview for the host's live edit-buffer text — see
/// `Session::nudge_repr`. Used by the Web/touch wheel/swipe nudge while
/// inline-editing a number: the host writes the result into the focused
/// `<input>` without dispatching or re-rendering.
pub fn nudge_repr(&self, path: JsValue, text: &str, delta: i64) -> Result<JsValue, JsValue> {
    let path: Path = from_value(path).map_err(js_serde_error)?;
    match self.session.nudge_repr(&path, text, delta) {
        Some(s) => Ok(JsValue::from_str(&s)),
        None => Ok(JsValue::UNDEFINED),
    }
}
```
Confirm `Path`, `from_value`, `js_serde_error` are already imported/in scope in this file (they are, used by `schema_hint` at line 138-140) before writing.

Add a smoke-test block in `crates/confy-ffi/functional_smoke.mjs`, immediately after the existing `// ---- 4. Nudge the integer ----` block (after line 62, before line 63's blank line / next section), reusing the existing `tuple`/`unit`/`check` helpers (lines 13-23) and the same `s` session used at line 60:
```js
// ---- 4b. nudge_repr previews without mutating ----
const beforeSerialize = s.serialize();
const preview = s.nudge_repr(s.snapshot().rows.find(r => r.key === "port").path, "8090", 1);
check("nudge_repr previews +1 from a given text", preview === "8091", preview);
check("nudge_repr does not mutate the document", s.serialize() === beforeSerialize);
```
Run `node crates/confy-ffi/functional_smoke.mjs` (after `wasm-pack build`/whatever existing build step the smoke test depends on — inspect the file's header comment for the required prior build command before running) to confirm this passes.

### 5. Web desktop: focus-armed wheel nudge on the tree's inline-edit `<input>` and the shared panel's value field

Both the tree's inline editor (`web/render.ts:86`, `<input data-editing="value">`) and the shared panel's value field (`web/panel.ts`, `[data-field="value"]`) need the same behavior: wheel only nudges while that specific input is focused, and once focused, a wheel event anywhere on the page (not just over the input) nudges it.

**`web/confy.ts`**: add a thin wrapper method on `Session` immediately after `schemaHint` (after line 121, mirroring its exact JSDoc/shape at lines 114-121):
```ts
/**
 * Stateless preview of nudging `text` by `delta` steps for `path` — does
 * not mutate the document. `undefined` when the node isn't a nudgeable
 * scalar or `text` doesn't parse. See `Session::nudge_repr`.
 */
nudgeRepr(path: Path, text: string, delta: number): string | undefined {
  return this.raw.nudge_repr(path, text, delta) as string | undefined;
}
```
Confirm `Path` is already imported in `confy.ts` (it's used by `schemaHint`'s signature at line 119) before writing.

**`web/ui.ts`**: add a new function `wireValueNudgeWheel(input: HTMLInputElement, row: ViewRow)` near `focusInlineEdit` (after its definition, current lines 1395-1403 — re-read exact end line before inserting since step 2 already removed lines above it and renumbered the file). Wire it from inside `focusInlineEdit`, right after `input.focus()` and the caret-position lines (currently lines 1400-1402), for the case where `row.scalar_type` is `"Integer"` or `"Float"` — read `focusInlineEdit`'s current body in full after step 2's edit lands (it calls `getEdit()` which returns `{ path, field, buffer, ... }`; the row for `edit.path` must be looked up the same way `onTreeWheel` used to, via `snap?.rows.find(...)`, since `getEdit()` doesn't carry `scalar_type`).

Function behavior:
```ts
// While `input` (the tree's inline value editor) is focused, capture every
// wheel tick anywhere on the page — not just while the pointer sits over
// the input — and write the nudged text straight into the input via a
// stateless core query. No Intent is dispatched, no re-render happens, and
// the caret/focus are never disturbed; the value only lands in the
// document on the normal commit path (Enter/blur -> CommitEdit). Torn down
// on blur so a later wheel scrolls the page normally again.
function wireValueNudgeWheel(input: HTMLInputElement, path: Path): void {
  const onWheel = (e: WheelEvent) => {
    e.preventDefault();
    const delta = e.deltaY < 0 ? 1 : -1;
    const next = session!.nudgeRepr(path, input.value, delta);
    if (next === undefined) return;
    input.value = next;
    const n = input.value.length;
    input.setSelectionRange(n, n);
  };
  window.addEventListener("wheel", onWheel, { passive: false, capture: true });
  input.addEventListener(
    "blur",
    () => window.removeEventListener("wheel", onWheel, { capture: true }),
    { once: true },
  );
}
```
Call it only for `Integer`/`Float` rows (never Bool — Bool has no free-text inline input; it opens `Mode::SchemaEnum` instead, per step 1). Read `session` (the module-level `Session` instance `ui.ts` already holds — grep its exact name before use; it is referenced elsewhere in this file for `session.dispatch`/similar wasm calls) to confirm the exact variable name before wiring.

**`web/panel.ts`**: replace the existing wheel block (lines 340-352, the `ve.addEventListener("wheel", ...)` call) — remove `"Bool"` from the type gate at line 339 (`if (st === "Bool" || st === "Integer" || st === "Float")` → `if (st === "Integer" || st === "Float")`), and replace the wheel handler body so it: (a) does nothing until `ve` is focused, (b) once focused, listens on `document` in the capture phase so it fires regardless of pointer position, (c) writes into `ve.value` via a new `nudgeRepr` param (passed into `wirePanel`, since `panel.ts` has no direct `Session` import — see signature change below) instead of calling `fire({ Nudge: ... })`, (d) tears down on blur.

`wirePanel`'s exported signature (`web/panel.ts:266-274`) gains one new parameter, `nudgeRepr: (path: Path, text: string, delta: number) => string | undefined`, inserted after `send` (so callers pass it positionally where `openKind` currently sits — shift `openKind`, `onError`, `batch`, `schemaEnum` each one position later):
```ts
export function wirePanel(
  container: HTMLElement,
  row: ViewRow,
  send: (intent: Intent) => SessionSnapshot,
  nudgeRepr: (path: Path, text: string, delta: number) => string | undefined,
  openKind: (row: ViewRow) => void,
  onError: (msg: string) => void,
  batch?: (fn: () => void) => void,
  schemaEnum?: { options: string[]; cursor: number },
): void {
```
Update both call sites to pass the new argument in the new position:
- `web/ui.ts:601` (`wirePanel(body, ...)` inside the desktop detail-panel renderer) — pass `(path, text, delta) => session!.nudgeRepr(path, text, delta)` (confirm exact call-site argument list by reading `web/ui.ts` around line 598-610 in full before editing, since line numbers will have shifted from step 2's deletions).
- The touch equivalent call site in `web/touch/app.ts` (grep `wirePanel(` in that file — not yet read; locate and read its exact call before editing) — pass the touch app's own `Session` handle's `nudgeRepr` the same way.

New wheel block in `panel.ts` (replacing lines 340-352):
```ts
    // Mouse-wheel nudges the value only once the field is focused (entering
    // inline-edit); once armed, every wheel tick anywhere on the page nudges
    // it (not just while the pointer hovers the field) until it blurs. No
    // Intent dispatch, no re-render — the nudged text is written straight
    // into `ve` and only committed via the normal Enter/blur `commit` path.
    if (st === "Integer" || st === "Float") {
      let onWheel: ((e: WheelEvent) => void) | null = null;
      ve.addEventListener("focus", () => {
        onWheel = (e: WheelEvent) => {
          e.preventDefault();
          const next = nudgeRepr(path, ve.value, e.deltaY < 0 ? 1 : -1);
          if (next === undefined) return;
          ve.value = next;
          const n = ve.value.length;
          ve.setSelectionRange(n, n);
        };
        document.addEventListener("wheel", onWheel, { passive: false, capture: true });
      });
      ve.addEventListener("blur", () => {
        if (onWheel) document.removeEventListener("wheel", onWheel, { capture: true });
        onWheel = null;
      });
    }
```

### 6. Touch: swipe-nudge only while the value field is focused, captured document-wide

`web/panel.ts`, the touch swipe-to-nudge block (lines 32-88 module state + lines 354-374 wiring):

- Delete the `document.activeElement === ve` early-return guard at line 362 (`if (document.activeElement === ve) return; // already editing — don't hijack native caret/selection`) — inverted: the gesture must now require focus, not avoid it.
- Change the `pointerdown` gate at line 360-374 from starting the gesture on `ve`'s own `pointerdown` to starting it only when `ve` is already the focused/active element, and additionally arm it from `document`-level `pointerdown` (capture phase) rather than `ve`'s own listener, so a touch that starts anywhere (not just on the input) while it's focused begins tracking. Concretely, replace the `ve.addEventListener("pointerdown", ...)` block (lines 360-373) with:
  ```ts
      ve.style.touchAction = "pan-y"; // let vertical scroll pass through natively; only horizontal is intercepted below
      document.addEventListener(
        "pointerdown",
        (e) => {
          if (e.pointerType !== "touch") return; // desktop mouse drag keeps native text selection
          if (document.activeElement !== ve) return; // only while this field is the active inline edit
          installValueNudgeListeners();
          nudgeGesture = {
            pointerId: e.pointerId,
            originX: e.clientX,
            originY: e.clientY,
            lastStep: 0,
            engaged: false,
            path,
            fire,
          };
        },
        { capture: true },
      );
  ```
  Note: this `document.addEventListener` is added once per `wirePanel` call (i.e. once per panel render, same lifecycle as the rest of this function's listeners) — do not attempt to dedupe/guard it the way `installValueNudgeListeners()` guards its own module-scope listeners; `wirePanel` already re-wires all its listeners on every render onto a freshly-rendered `container`/`ve`, so a stale prior listener referencing a detached `ve` is harmless (it will fail the `document.activeElement !== ve` check as soon as focus moves) but MUST still be checked against the existing `nudgeListenersWired` pattern (lines 56-59) — if `installValueNudgeListeners` is idempotent (it is, guarded by `nudgeListenersWired`), no further dedup is needed here since the `pointerdown` closure captures the current render's own `path`/`fire`/`ve` and only matches while `document.activeElement === ve` for *that* render's `ve`.

- The `nudgeGesture.fire` calls inside `installValueNudgeListeners`'s `pointermove` handler (lines 60-80) currently do `fire({ SetCursor: path }); fire({ Nudge: delta });` (lines 76-77) — this still dispatches `Intent::Nudge` through core (mutating the document per tick), which is inconsistent with the "no per-tick mutation" design adopted for wheel in steps 3-5. Replace this with the same `nudgeRepr`-and-write-into-input approach used for wheel: change `ValueNudgeGesture` (lines 46-54) to drop `path`/`fire` in favor of `input: HTMLInputElement` and `nudgeRepr: (path: Path, text: string, delta: number) => string | undefined` plus `path: Path`, and change lines 71-77 from:
  ```ts
      const step = Math.trunc(dx / VALUE_NUDGE_STEP_PX);
      if (step === nudgeGesture.lastStep) return;
      const delta = step - nudgeGesture.lastStep;
      nudgeGesture.lastStep = step;
      const { path, fire } = nudgeGesture;
      fire({ SetCursor: path });
      fire({ Nudge: delta });
  ```
  to:
  ```ts
      const step = Math.trunc(dx / VALUE_NUDGE_STEP_PX);
      if (step === nudgeGesture.lastStep) return;
      const delta = step - nudgeGesture.lastStep;
      nudgeGesture.lastStep = step;
      const { input, path, nudgeRepr } = nudgeGesture;
      const next = nudgeRepr(path, input.value, delta);
      if (next === undefined) return;
      input.value = next;
      const n = input.value.length;
      input.setSelectionRange(n, n);
  ```
  Update the `nudgeGesture = { ... }` construction site (in the `document.addEventListener("pointerdown", ...)` block written above) to pass `input: ve, nudgeRepr` instead of `fire`.
  Update the `ValueNudgeGesture` interface (lines 46-54) accordingly: remove `fire: (intent: Intent) => void`, add `input: HTMLElement` (typed `HTMLInputElement`) and `nudgeRepr: (path: Path, text: string, delta: number) => string | undefined`.

- Update the module doc comment above this feature (lines 32-43) to reflect the new trigger (focused, not idle) and the no-mutation-until-commit design — rewrite it in place; do not leave stale prose describing "an *idle* (unfocused)" field.

### 7. Documentation and i18n (mechanical, but load-bearing content — not cleanup)

These describe removed/changed behavior and must match the new behavior exactly:

- `i18n/en.json` lines 105-107 (`tui.help.toml`/`tui.help.json`/`tui.help.yaml`): change `"←/→          Toggle bool / ±1 number    a   Add node\n"` to `"←/→          ±1 number             a   Add node\n"` in all three keys (keep column alignment consistent with the surrounding lines in that same string — read each full string before editing to match spacing).
- `i18n/zh-TW.json` lines 105-107 (same three keys): change `"←/→          切換布林...` (read the full existing string first — it was truncated in the earlier grep) to drop the bool-toggle clause, keeping the ±1 number clause, mirroring the English edit's structure.
- `docs/reference/TUI.md` line 80: `` `←/→` still toggles a bool in place without any popup. `` — delete this sentence (bool nudging removed; the preceding sentence about `Tab` typing bool free-form is unaffected and stays).
- `docs/reference/WEBUI.md` lines 292-301: rewrite this paragraph — remove the "Mouse-wheel over the value cell... a `Bool` toggles true↔false" claim and the touch "unfocused" swipe claim; describe the new behavior: wheel/swipe nudge Integer/Float only after the field has entered inline-edit (focus), captured globally until blur/Enter/Escape; Bool has no wheel/swipe/arrow-key affordance, only the true/false picker sheet/select.
- `web/help-content.ts` lines 11-12 and 56-57: `+/- or ←/→     nudge numeric value` — already bool-silent (says "numeric value", not "bool"), confirm no edit needed here; leave as-is (verify by re-reading both lines in full context before concluding no change is needed).

## Critical files & anchors

- `crates/confy-core/src/session/schema_hint.rs:7-15` (`nudge_scalar`) — the single core chokepoint; removing the Bool arm here is what silently disables every keyboard/FFI bool-nudge path.
- `crates/confy-core/src/session/inline_edit.rs:758-878` (`schema_clamp_nudge` + `nudge`) — `nudge_repr` (step 3) must sit beside these and reuse both without duplicating the clamp logic.
- `web/panel.ts:32-88, 328-374` — both the wheel gate and the swipe-nudge gesture state machine live here; this file has the most structurally sensitive edit (interface change, gesture arm/disarm inversion).
- `web/ui.ts:1358-1403, 1902-1903` — `toggleBool`/`onTreeWheel` deletion and `focusInlineEdit` wiring; re-read this region fresh after step 2's deletion since line numbers shift before doing step 5's insertion.
- `crates/confy-ffi/src/lib.rs:137-141` (`schema_hint`) — exact pattern to mirror for the new `nudge_repr` FFI export (`from_value`/`js_serde_error`, `Result<JsValue, JsValue>` shape).

## Verification

1. `cargo test -p confy-core` — `nudge_scalar_steps_each_type_preserving_format` (Bool now asserts `None`) and new `nudge_repr_previews_without_mutating` pass; full existing suite (including `nudge_reapplies_underscore_grouping`, `schema_clamp_nudge_snaps_to_multiple_of_and_clamps_to_bounds`, `crates/confy-core/tests/modal_lock.rs::nudge_locked_while_clipboard_armed`, `crates/confy-core/tests/schema_headless.rs::dispatch_nudge_clamps_to_schema_maximum`, `crates/confy-core/tests/session_headless.rs::dispatch_nudge_increments_scalar_via_snapshot`) still passes unmodified (these all use Integer, untouched by the Bool removal).
2. `cargo test -p confy-tui` — `nudge_writes_back_through_replace`, `yaml_nudge_preserves_inline_comment`, `json_nudge_integer_commits` (all Integer-based) still pass unmodified.
3. Build the wasm FFI per the existing project build step (read `crates/confy-ffi/functional_smoke.mjs`'s header for the exact prerequisite command) then `node crates/confy-ffi/functional_smoke.mjs` — all existing checks plus the new `nudge_repr` checks (step 4) pass.
4. `node web/build.mjs` (or the project's existing web build command — confirm exact invocation from `web/cf-build.sh`/`package.json` before running) succeeds with no TypeScript errors from the `wirePanel` signature change (both call sites updated) and the `ValueNudgeGesture` interface change.
5. Manual real-build check (desktop web, per this repo's Bug-Fix Protocol — a synthetic DOM test is not sufficient for a focus/gesture behavior change): load a TOML doc with an integer and a bool in the actual web UI (`file://` or dev server per `web/index.html`), then confirm:
   - Hovering a bool row's value and scrolling the wheel: no change (previously toggled).
   - Pressing `←/→` on a bool row: no change (previously toggled); the row still opens the true/false picker via `e`/click.
   - Hovering an integer row's value (tree, not yet in edit mode) and scrolling: no change (previously nudged ±1).
   - Double-click/`e` into the integer's inline edit input, then scroll the wheel while the pointer is *elsewhere on the page* (not over the input): the value in the input changes ±1 per tick, caret stays at end, input stays focused, no undo entry is created yet.
   - Press Enter: value commits, exactly one new undo entry appears (`Intent::Undo` reverts the whole nudge session in one step, not one step per tick).
   - Repeat the open-panel/detail-aside equivalent (`i` or click a row to open the shared panel) for both wheel (desktop) and swipe (touch emulation via Chromium DevTools touch simulation) — swipe must not nudge until the field is focused, and once focused, a horizontal swipe starting outside the input still nudges it.

## Assumptions & contingencies

- Assumed the existing `nudge_scalar` match in `schema_hint.rs` has a catch-all wildcard arm already returning `None` for non-numeric types (String/Datetime) that `Bool` can simply join, based on the test comment "strings / datetimes are not nudgeable" at `session.rs:2186`. If instead the match lists each type explicitly with no wildcard, add `ScalarType::Bool => None,` as its own arm rather than relying on a wildcard — re-read the full match body before editing to pick whichever form fits without introducing an unreachable-pattern warning.
- Assumed `web/touch/app.ts` calls `wirePanel(...)` with the same positional-argument convention as `web/ui.ts` (per the shared-panel design noted in `panel.ts`'s own header comment) and holds its own `Session`/wasm handle capable of exposing `nudgeRepr` the same way `web/confy.ts`'s `Session.nudgeRepr` does. If touch's session handle is structured differently (e.g., a different wrapper type without a `raw.nudge_repr` passthrough), add the equivalent thin wrapper method there before wiring the call site, mirroring step 5's `confy.ts` addition exactly.
