// Pure "which Intent does this (mode, key) pair mean" resolution, extracted
// from `ui.ts`'s `onKey` (see
// docs/superpowers/plans/2026-08-11-web-code-audit-remediation-plan.md,
// Task 8). Mirrors `onKey`'s branch structure and mode-precedence exactly
// (Edit > Prompt > Convert > TypeFilter > KindSwitch > AddPicker > ActionMenu > SchemaEnum > Help >
// tree shortcuts) so it's unit-testable without a DOM, same pattern as
// `toolbar-fold.ts`'s pure-logic extraction.
//
// `onKey` itself keeps every actual side effect: `session`/`snap` null and
// modal-open guards (checked before calling this), `ev.preventDefault()`,
// and the handful of branches that are NOT a single `Intent` dispatch —
// `navSelect()` (dispatches the nav intent *and* a conditional
// `SetSelection`), `toggleSelectedBranches()` (reads live `snap`, dispatches
// a variable intent set), `doSave()`/`doOpen()` (async host I/O),
// `runSaveConvertShared()` (the whole save-convert flow), and
// `typeFilterPageStep()` (reads `document.getElementById("tfPop")` — real
// DOM, can't live in a pure function). Those are represented as tagged
// `"nav"`/`"native"`/`"typefilter-page"` results instead of a plain intent;
// `onKey` switches on the tag to call the right host function, unchanged
// from before the extraction.
import type { Intent, ModeView, TypeFilterView } from "./types.js";
import { modeTag } from "./mode.js";

export type KeyResolution =
  | { kind: "intent"; intent: Intent; preventDefault: boolean }
  // j/k/g/G (and their Arrow/Home/End equivalents): dispatched via
  // `navSelect()`, not a plain `send()` — see module doc above.
  | { kind: "nav"; intent: Intent; preventDefault: boolean }
  // PageUp/PageDown in TypeFilter mode: the page size is DOM-derived
  // (`typeFilterPageStep`), so only the direction is pure; `onKey` always
  // calls `ev.preventDefault()` for this result, matching the original.
  | { kind: "typefilter-page"; dir: -1 | 1 }
  // PageUp/PageDown in the tree (normal mode): page size is DOM-derived
  // (`treePageStep`), so only direction is pure — same split as
  // `typefilter-page`. Host always calls `ev.preventDefault()`.
  | { kind: "tree-page"; dir: -1 | 1 }
  | {
      kind: "native";
      action: "focus-search" | "undo" | "redo" | "save" | "open" | "toggle-branches" | "save-convert";
      preventDefault: boolean;
    }
  | null;

// Count of navigable (non-header) TypeFilter rows — used to compute Home/End
// jump deltas. Pure (only reads `grid.rows`), unlike `typeFilterPageStep`
// (`ui.ts`), which additionally reads the popup's rendered DOM size. Also
// used by `ui.ts` itself (`typeFilterPageStep`) — exported to avoid a
// duplicate definition.
export function navRowCount(grid: TypeFilterView): number {
  return grid.rows.filter((r) => "Cells" in r).length;
}

// PageUp/PageDown step for the tree, in visible-row units — mirrors the
// TUI's `terminal_height / 2` convention (crates/confy-tui/src/tui/mod.rs)
// without assuming a fixed row height: derive the on-screen row count from
// the scroll-container ratio (same technique as `typeFilterPageStep`), then
// halve it. `totalRows` is `snap.rows.length`; `clientH`/`scrollH` are the
// tree scroll container's `clientHeight`/`scrollHeight`.
export function treePageStep(totalRows: number, clientH: number, scrollH: number): number {
  if (totalRows === 0) return 1;
  const ratio = scrollH > 0 ? clientH / scrollH : 1;
  const visible = Math.max(1, Math.min(totalRows, Math.round(ratio * totalRows)));
  return Math.max(1, Math.floor(visible / 2));
}

// PageUp/PageDown stride for the Action menu popup — eight items, always
// fully visible, so a fixed step (same convention as SchemaEnum's page below)
// rather than a DOM-measured screenful.
const ACTION_MENU_PAGE_STEP = 5;

/** Home/End deltas for the Action menu. Core's `action_menu_move` strides by
 * `delta` modulo `items.length` (skipping disabled), so a SchemaEnum-style
 * `-items.length` would wrap back to a no-op — landing on the first/last
 * *enabled* item takes the exact offset `target − cursor`, which the stride
 * loop reaches in one hop. */
function actionMenuEdgeDelta(
  am: { cursor: number; items: { enabled: boolean }[] },
  last: boolean,
): number {
  const idx = last
    ? am.items.map((it) => it.enabled).lastIndexOf(true)
    : am.items.findIndex((it) => it.enabled);
  return idx - am.cursor;
}


export function resolveKeyIntent(
  mode: ModeView,
  key: string,
  mods: { ctrl: boolean; shift: boolean },
  rawView: boolean,
  vshost: boolean,
): KeyResolution {
  const m = mode;
  if (typeof m === "object" && "Edit" in m) {
    if (key === "Enter") return { kind: "intent", intent: "EditCommit", preventDefault: false };
    if (key === "Escape") return { kind: "intent", intent: "EditCancel", preventDefault: false };
    if (key === "Tab") return { kind: "intent", intent: "EditToggleField", preventDefault: true };
    if (key === "Backspace") return { kind: "intent", intent: "EditBackspace", preventDefault: false };
    if (key.length === 1) return { kind: "intent", intent: { EditChar: key }, preventDefault: false };
    return null;
  }
  if (typeof m === "object" && "Prompt" in m) {
    if (key === "y" || key === "Y" || key === "Enter")
      return { kind: "intent", intent: { PromptKey: "y" }, preventDefault: false };
    if (key === "n" || key === "N" || key === "Escape")
      return { kind: "intent", intent: { PromptKey: "n" }, preventDefault: false };
    // Collision offers Overwrite (o) / Rename (r) besides cancel.
    if (key === "o" || key === "r") return { kind: "intent", intent: { PromptKey: key }, preventDefault: false };
    return null;
  }
  if (typeof m === "object" && "Convert" in m) {
    const step = m.Convert.step;
    if (key === "Escape") return { kind: "intent", intent: "Escape", preventDefault: false };
    if (step === "Format") {
      if (key === "ArrowUp") return { kind: "intent", intent: { ConvertMove: -1 }, preventDefault: false };
      if (key === "ArrowDown") return { kind: "intent", intent: { ConvertMove: 1 }, preventDefault: false };
      if (key === "Enter") return { kind: "intent", intent: "ConvertPickFormat", preventDefault: false };
    } else if (step === "Path") {
      if (key === "Enter") return { kind: "native", action: "save-convert", preventDefault: false };
      if (key === "Backspace") return { kind: "intent", intent: "ConvertPathBackspace", preventDefault: false };
      if (key.length === 1) return { kind: "intent", intent: { ConvertPathChar: key }, preventDefault: false };
    } else if (step === "Confirm") {
      if (key === "y" || key === "Y" || key === "Enter")
        return { kind: "intent", intent: "ConvertConfirm", preventDefault: false };
      return { kind: "intent", intent: "Escape", preventDefault: false };
    }
    return null;
  }
  if (typeof m === "object" && "TypeFilter" in m) {
    const grid = m.TypeFilter;
    if (key === "ArrowUp") return { kind: "intent", intent: { TypeFilterMove: [-1, 0] }, preventDefault: false };
    if (key === "ArrowDown") return { kind: "intent", intent: { TypeFilterMove: [1, 0] }, preventDefault: false };
    if (key === "ArrowLeft") return { kind: "intent", intent: { TypeFilterMove: [0, -1] }, preventDefault: false };
    if (key === "ArrowRight") return { kind: "intent", intent: { TypeFilterMove: [0, 1] }, preventDefault: false };
    if (key === "Home") return { kind: "intent", intent: { TypeFilterMove: [-navRowCount(grid), 0] }, preventDefault: false };
    if (key === "End") return { kind: "intent", intent: { TypeFilterMove: [navRowCount(grid), 0] }, preventDefault: false };
    if (key === "PageUp") return { kind: "typefilter-page", dir: -1 };
    if (key === "PageDown") return { kind: "typefilter-page", dir: 1 };
    if (key === " ") return { kind: "intent", intent: "TypeFilterToggle", preventDefault: true };
    if (key === "Enter") return { kind: "intent", intent: "CommitTypeFilter", preventDefault: false };
    if (key === "Escape") return { kind: "intent", intent: "ExitTypeFilter", preventDefault: false };
    return null;
  }
  if (modeTag(m) === "KindSwitch") {
    if (key === "ArrowUp") return { kind: "intent", intent: { KindSwitchMove: -1 }, preventDefault: false };
    if (key === "ArrowDown") return { kind: "intent", intent: { KindSwitchMove: 1 }, preventDefault: false };
    if (key === "Enter") return { kind: "intent", intent: "KindSwitchCommit", preventDefault: false };
    if (key === "Escape") return { kind: "intent", intent: "ExitKindSwitch", preventDefault: false };
    return null;
  }
  if (typeof m === "object" && "AddPicker" in m) {
    const st = m.AddPicker;
    const ADD_PICKER_PAGE_STEP = 5;
    if (key === "ArrowUp") return { kind: "intent", intent: { AddPickerMove: -1 }, preventDefault: true };
    if (key === "ArrowDown") return { kind: "intent", intent: { AddPickerMove: 1 }, preventDefault: true };
    if (key === "Home") return { kind: "intent", intent: { AddPickerJump: -st.options.length }, preventDefault: true };
    if (key === "End") return { kind: "intent", intent: { AddPickerJump: st.options.length }, preventDefault: true };
    if (key === "PageUp") return { kind: "intent", intent: { AddPickerJump: -ADD_PICKER_PAGE_STEP }, preventDefault: true };
    if (key === "PageDown") return { kind: "intent", intent: { AddPickerJump: ADD_PICKER_PAGE_STEP }, preventDefault: true };
    if (key === "Enter") return { kind: "intent", intent: "AddPickerCommit", preventDefault: false };
    if (key === "Escape") return { kind: "intent", intent: "ExitAddPicker", preventDefault: false };
    return null;
  }
  if (typeof m === "object" && "ActionMenu" in m) {
    const am = m.ActionMenu;
    if (key === "ArrowUp") return { kind: "intent", intent: { ActionMenuMove: -1 }, preventDefault: false };
    if (key === "ArrowDown") return { kind: "intent", intent: { ActionMenuMove: 1 }, preventDefault: false };
    if (key === "Home") return { kind: "intent", intent: { ActionMenuMove: actionMenuEdgeDelta(am, false) }, preventDefault: true };
    if (key === "End") return { kind: "intent", intent: { ActionMenuMove: actionMenuEdgeDelta(am, true) }, preventDefault: true };
    if (key === "PageUp") return { kind: "intent", intent: { ActionMenuMove: -ACTION_MENU_PAGE_STEP }, preventDefault: true };
    if (key === "PageDown") return { kind: "intent", intent: { ActionMenuMove: ACTION_MENU_PAGE_STEP }, preventDefault: true };
    if (key === "Enter") return { kind: "intent", intent: "ActionMenuCommit", preventDefault: false };
    if (key === "Escape") return { kind: "intent", intent: "Escape", preventDefault: false };
    return null;
  }
  if (typeof m === "object" && "SchemaEnum" in m) {
    const st = m.SchemaEnum;
    const SCHEMA_ENUM_PAGE_STEP = 5;
    if (key === "ArrowUp") return { kind: "intent", intent: { SchemaEnumMove: -1 }, preventDefault: true };
    if (key === "ArrowDown") return { kind: "intent", intent: { SchemaEnumMove: 1 }, preventDefault: true };
    if (key === "Home") return { kind: "intent", intent: { SchemaEnumJump: -st.options.length }, preventDefault: true };
    if (key === "End") return { kind: "intent", intent: { SchemaEnumJump: st.options.length }, preventDefault: true };
    if (key === "PageUp") return { kind: "intent", intent: { SchemaEnumJump: -SCHEMA_ENUM_PAGE_STEP }, preventDefault: true };
    if (key === "PageDown") return { kind: "intent", intent: { SchemaEnumJump: SCHEMA_ENUM_PAGE_STEP }, preventDefault: true };
    if (key === "Enter") return { kind: "intent", intent: "SchemaEnumCommit", preventDefault: false };
    return null;
  }
  // Help/About panel: pause every tree shortcut (only close/tab-switch handled).
  if (modeTag(m) === "Help") {
    if (key === "Escape" || key === "?") return { kind: "intent", intent: "Escape", preventDefault: false };
    if (key === "Tab") return { kind: "intent", intent: "ToggleHelpTab", preventDefault: true };
    return null;
  }

  const { ctrl, shift } = mods;
  if (ctrl && key === "s") return { kind: "native", action: "save", preventDefault: true };
  if (ctrl && key === "o") return { kind: "native", action: "open", preventDefault: true };
  if (ctrl) return null;
  if (rawView) return null;
  if (shift && (key === "ArrowUp" || key === "ArrowDown")) {
    return {
      kind: "intent",
      intent: key === "ArrowUp" ? "ExtendSelectUp" : "ExtendSelectDown",
      preventDefault: true,
    };
  }
  // Arrows / Home / End / Space natively scroll the focused container; `onKey`
  // owns them as navigation — every key in this list gets `preventDefault`
  // regardless of which case below matches (mirrors the original's
  // unconditional pre-switch check).
  const preSwitchPD = ["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Home", "End", " "].includes(key);
  switch (key) {
    case "j": case "ArrowDown": return { kind: "nav", intent: "CursorDown", preventDefault: preSwitchPD };
    case "k": case "ArrowUp": return { kind: "nav", intent: "CursorUp", preventDefault: preSwitchPD };
    case "g": case "Home": return { kind: "nav", intent: "CursorHome", preventDefault: preSwitchPD };
    case "G": case "End": return { kind: "nav", intent: "CursorEnd", preventDefault: preSwitchPD };
    case "PageUp": return { kind: "tree-page", dir: -1 };
    case "PageDown": return { kind: "tree-page", dir: 1 };
    case "Enter": return { kind: "intent", intent: "ToggleDetail", preventDefault: false };
    case " ": return { kind: "native", action: "toggle-branches", preventDefault: preSwitchPD };
    // preventDefault: these open a text editor synchronously (inline input or the
    // external modal); without it the triggering keystroke leaks into the field.
    case "e": return { kind: "intent", intent: "BeginEdit", preventDefault: true };
    // F2 rename — mirrors the TUI's `KeyCode::F(2)` binding (crates/confy-tui/src/tui/keys.rs).
    case "F2": return { kind: "intent", intent: "BeginRename", preventDefault: true };
    case "a": return { kind: "intent", intent: "AddNode", preventDefault: true };
    case "d": case "Delete": return { kind: "intent", intent: "DeleteSelected", preventDefault: false };
    case "c": return { kind: "intent", intent: "CopySelected", preventDefault: false };
    case "x": return { kind: "intent", intent: "CutSelected", preventDefault: false };
    case "v": return { kind: "intent", intent: "Paste", preventDefault: false };
    case "r": return { kind: "intent", intent: "Remark", preventDefault: false };
    case "z": return { kind: "native", action: "undo", preventDefault: false };
    case "y": return { kind: "native", action: "redo", preventDefault: false };
    case "s": return { kind: "intent", intent: "ToggleSelect", preventDefault: false };
    case "1": return { kind: "intent", intent: "ExpandLevel", preventDefault: false };
    case "2": return { kind: "intent", intent: "CollapseLevel", preventDefault: false };
    case "0": return { kind: "intent", intent: "CollapseAll", preventDefault: false };
    case "9": return { kind: "intent", intent: "ExpandAll", preventDefault: false };
    case "+": case "ArrowRight": return { kind: "intent", intent: { Nudge: 1 }, preventDefault: preSwitchPD };
    case "-": case "ArrowLeft": return { kind: "intent", intent: { Nudge: -1 }, preventDefault: preSwitchPD };
    case "/": return { kind: "native", action: "focus-search", preventDefault: true };
    case "f": return { kind: "intent", intent: "EnterTypeFilter", preventDefault: false };
    case "K": return { kind: "intent", intent: "OpenKindSwitch", preventDefault: false };
    case "m": return { kind: "intent", intent: "OpenActionMenu", preventDefault: false };
    case "C": return { kind: "intent", intent: "OpenConvert", preventDefault: false };
    case "i": return { kind: "intent", intent: "ToggleDetail", preventDefault: false };
    case "?": return { kind: "intent", intent: "EnterHelp", preventDefault: false };
    case "Escape": return { kind: "intent", intent: "Escape", preventDefault: false };
    case "q": return vshost ? null : { kind: "intent", intent: "QuitRequested", preventDefault: false };
  }
  return null;
}
