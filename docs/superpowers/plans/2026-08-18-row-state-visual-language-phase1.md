✅ **Shipped — historical reference.** See `CHANGELOG.md` for current behavior; this plan is kept for context, not as a live task list.

# Row-State Visual Language (Phase 1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the three background-fill row states (Cursor, Cut source, Copy source) mutually
exclusive and identically colored across TUI/desktop/touch, and demote Locked selection from a
background fill to a leading marker everywhere, per ADR 0005 §2.

**Architecture:** Three independent per-surface changes, no shared runtime code (each surface
already computes `is_cursor`/`selected`/clipboard-source membership from the same core snapshot
fields — see ADR 0005 §1 — this phase only changes how each surface *paints* those existing
booleans). TUI: `Style` selection in `crates/confy-tui/src/tui/ui.rs`'s row-render loop. Desktop:
CSS only (`web/style.css`) — the `cursor`/`selected`/`clip-copy`/`clip-cut` classes are already
emitted by `web/render.ts`, only their rules change. Touch: `web/touch/render.ts` gains the
`clip-copy`/`clip-cut` class emission desktop already has, plus matching CSS in
`web/touch/style.css`.

**Tech Stack:** Rust + ratatui 0.28 (TUI), TypeScript + esbuild + plain CSS custom properties, no
framework (web/desktop, web/touch).

**Spec:** `ROW_STATE_MODEL.md` §3 (visual design table), `docs/adr/0005-row-cursor-selection-clipboard-state-model.md` §2.

## Global Constraints

- Final color assignment (from the original request, confirmed in ADR 0005): Cursor = blue,
  Cut source = green, Copy source = purple/magenta. These three are mutually exclusive
  full-row background fills — a row shows at most one.
- Locked selection gets **no background fill anywhere** — TUI already has its marker (the `●`
  glyph, `tui/ui.rs:328-333`, pre-existing, untouched by this phase); desktop/touch get a new
  leading `::before` bar marker, colored with the existing `--sel-edge` token (already identical
  in both stylesheets — do not invent a new color for it).
- Do not touch `--sel` (still used by the unrelated breadcrumb-current-row highlight,
  `web/style.css:717`) — add new, additively-named custom properties instead of repurposing it.
- Dead `.row.cut` CSS rules (`web/style.css:169`, `web/touch/style.css:119` — no code path ever
  emits a bare `cut` class, only `clip-cut`) are removed as part of this phase, not carried
  forward.
- No `Intent`/wire-format/snapshot changes. Pure rendering.

---

### Task 1: TUI — drop the selection background fill, fix the copy/cut color collision

**Files:**
- Modify: `crates/confy-tui/src/tui/ui.rs:379-392` (row style selection), append test to the
  existing `mod tests` block (same file).

**Interfaces:**
- Consumes: `app.session.selection: Selection` (`.contains(&Path) -> bool`), `app.session.clipboard: Option<Clipboard>` (`.cut: bool`, `.sources: Vec<Path>`) — both pre-existing, unchanged.
- Produces: no new public API. Later tasks (2, 3) do not depend on this one — independent files, independent languages.

- [ ] **Step 1: Write the failing tests**

Append inside the existing `#[cfg(test)] mod tests { ... }` block in
`crates/confy-tui/src/tui/ui.rs` (after the last existing `#[test]` fn, before the module's
closing `}`):

```rust
#[test]
fn cursor_selection_and_clip_source_colors_are_distinct_and_composable() {
    let doc = crate::model::any_doc::AnyDocument::Toml(
        crate::model::cst_doc::CstDocument::from_str("a = 1\nb = 2\nc = 3\n").unwrap(),
    );
    let mut app = App::new(doc);
    app.select_row(2); // rows[0] is the root; rows[1]=a, rows[2]=b, rows[3]=c — cursor on `b`
    app.session.selection.toggle(app.row_path(2)); // lock-select the cursor row too
    app.session.selection.toggle(app.row_path(3)); // and a second, non-cursor row (`c`)
    let mut terminal = Terminal::new(TestBackend::new(40, 8)).unwrap();
    terminal.draw(|fr| draw(fr, &app)).unwrap();
    let buf = terminal.backend().buffer().clone();
    let row_y = |needle: &str| -> u16 {
        (0..8)
            .find(|&y| (0..40).any(|x| buf[(x, y)].symbol() == needle))
            .unwrap_or_else(|| panic!("row containing {needle:?} not found in rendered buffer"))
    };
    let cursor_y = row_y("b");
    let locked_only_y = row_y("c");
    assert_eq!(
        buf[(0, cursor_y)].bg,
        Color::Blue,
        "cursor row must be blue, not the retired grey selection fill"
    );
    assert!(
        (0..40).any(|x| buf[(x, cursor_y)].symbol() == "●"),
        "locked-selection glyph must still render on a row that is also the cursor"
    );
    assert_eq!(
        buf[(0, locked_only_y)].bg,
        Color::Reset,
        "a locked-selection row that is not the cursor must not paint any background fill"
    );
}

#[test]
fn clip_source_colors_do_not_collide_with_cursor_blue() {
    let doc = crate::model::any_doc::AnyDocument::Toml(
        crate::model::cst_doc::CstDocument::from_str("a = 1\nb = 2\n").unwrap(),
    );
    let mut app = App::new(doc);
    let a_path = app.row_path(1); // rows[0] is the root; `a` is the first real row
    app.session.clipboard = Some(Clipboard {
        fragments: vec!["a = 1\n".into()],
        cut: false,
        sources: vec![a_path],
    });
    let mut terminal = Terminal::new(TestBackend::new(40, 8)).unwrap();
    terminal.draw(|fr| draw(fr, &app)).unwrap();
    let buf = terminal.backend().buffer().clone();
    let row_y = (0..8)
        .find(|&y| (0..40).any(|x| buf[(x, y)].symbol() == "a"))
        .expect("copy-source row not found in rendered buffer");
    assert_eq!(
        buf[(0, row_y)].bg,
        Color::Magenta,
        "copy source must use its own color, not the cursor's blue"
    );
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p confy-tui --lib tui::ui::tests::cursor_selection_and_clip_source_colors_are_distinct_and_composable tui::ui::tests::clip_source_colors_do_not_collide_with_cursor_blue -- --nocapture`

Expected: FAIL — the first test fails on the `Color::Reset` assertion for the locked-only row
(currently `Color::DarkGray`); the second fails on `Color::Magenta` (currently `Color::Blue`).

- [ ] **Step 3: Implement**

Replace `crates/confy-tui/src/tui/ui.rs:379-392`:

```rust
            // Base (non-cursor) appearance: copy source purple, cut source green.
            // Locked selection no longer paints a background — its `sel_marker` glyph
            // (above) is the sole visual cue now, so it composes with the cursor's blue
            // and the clip-source colors instead of being hidden underneath a grey fill
            // (ADR 0005 §2 / ROW_STATE_MODEL.md §3).
            let base = if in_clipboard_source {
                let cut = app.session.clipboard.as_ref().is_some_and(|cb| cb.cut);
                let bg = if cut { Color::Green } else { Color::Magenta };
                Style::default().bg(bg).fg(Color::White)
            } else if row.violations.is_some() {
                // Subdued, not alarming — a soft constraint, never a hard error.
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            };
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p confy-tui --lib tui::ui::tests::cursor_selection_and_clip_source_colors_are_distinct_and_composable tui::ui::tests::clip_source_colors_do_not_collide_with_cursor_blue -- --nocapture`

Expected: PASS.

- [ ] **Step 5: Run the full TUI test suite (nothing else references the removed grey/blue assignment)**

Run: `cargo test -p confy-tui`

Expected: PASS. If any other existing test asserted `Color::DarkGray` for a selected row or
`Color::Blue` for a copy source, update that assertion to match the new behavior — it was
testing the state this phase deliberately changes, not a regression.

- [ ] **Step 6: Commit**

```bash
git add crates/confy-tui/src/tui/ui.rs
git commit -m "fix(tui): drop selection bg fill, stop copy-source colliding with cursor blue (ADR 0005 §2)"
```

---

### Task 2: Desktop — retire the cursor bar/selection ring, unify cursor/hover/clip-source fills

**Files:**
- Modify: `web/style.css:17-18` (light `:root` tokens), `web/style.css:42-43` (dark
  `:root[data-theme="dark"]` tokens — re-read the file first; insertions in Task 1's sibling
  file don't shift these line numbers, but confirm before editing), `web/style.css:166-169`
  (`.row:hover`/`.row.selected`/`.row.cursor::before`/`.row.cut`), `web/style.css:566-568`
  (`.row.clip-copy`/`.row.clip-cut`).

**Interfaces:**
- Consumes: `.row.cursor`, `.row.selected`, `.row.clip-copy`, `.row.clip-cut` classes — already
  emitted by `web/render.ts:101-105` (`is_cursor`→`cursor`, `selected`→`selected`, `clip` param
  →`clip-copy`/`clip-cut`). No TS change in this task.
- Produces: `--cursor-bg`, `--cut-bg`, `--copy-bg` custom properties — Task 3 defines the
  **same three names with the same values** in `web/touch/style.css`'s own `:root` blocks (the
  two stylesheets already duplicate every other color token verbatim, per existing convention —
  do not add a cross-file `@import`, that would be an unrelated, unrequested restructuring).

- [ ] **Step 1: Add the new tokens**

In `web/style.css`'s light `:root` block, immediately after the existing `--sel-edge:` line:

```css
  --cursor-bg: oklch(93% 0.05 250);
  --cut-bg:    oklch(93% 0.05 150);
  --copy-bg:   oklch(93% 0.05 300);
```

In `web/style.css`'s `:root[data-theme="dark"]` block, immediately after its own
`--sel-edge:` line:

```css
  --cursor-bg: oklch(34% 0.06 250);
  --cut-bg:    oklch(34% 0.06 150);
  --copy-bg:   oklch(34% 0.06 300);
```

(`--cursor-bg` intentionally equals the pre-existing `--sel` value in both themes — cursor is
taking over the "prominent light fill" role `--sel` used to play for selection. `--cut-bg`/
`--copy-bg` are the same lightness/chroma at `--t-string`'s hue (150, green) and `--t-date`'s hue
(300, purple) respectively, so they read as clearly related to — but distinct from — the existing
green/purple type-color vocabulary.)

- [ ] **Step 2: Retarget cursor/hover to a shared full-row fill; drop the cursor bar**

Replace `web/style.css:166-168`:

```css
.row:hover,.row.cursor{background:var(--cursor-bg)}
```

(deletes the old `.row.cursor::before{...background:var(--accent)}` bar rule entirely — its
`::before` slot is reused by `.row.selected` in Step 3.)

- [ ] **Step 3: Retarget `.row.selected` from a fill+ring to a leading bar marker**

Replace `web/style.css:167` (the old `.row.selected{background:var(--sel);...}` line — now at a
shifted line number after Step 2's edit; re-read before applying):

```css
.row.selected::before{content:"";position:absolute;left:2px;top:4px;bottom:4px;width:3px;border-radius:2px;background:var(--sel-edge)}
```

(`.row` already has `position:relative` — `web/style.css:163` — so this `::before` anchors
exactly like the old `.row.cursor::before` did.)

- [ ] **Step 4: Remove the dead `.row.cut` rule**

Delete the line `.row.cut{opacity:.45}` (originally `web/style.css:169`) — no code path ever
emits a bare `cut` class (only `clip-cut`).

- [ ] **Step 5: Swap and simplify clip-copy/clip-cut to full-row fills**

Replace `web/style.css:566-568`:

```css
.row.clip-cut { background: var(--cut-bg); }
.row.clip-copy { background: var(--copy-bg); }
```

(drops the old dashed-outline/opacity/strikethrough treatment — the solid fill is now the
signal, matching cursor's and matching cut=green/copy=purple from the original request. These
two rules must stay textually **after** the `.row.cursor`/`.row:hover` rule from Step 2, so a
row that is simultaneously the cursor and a clip source — e.g. cursor sitting on its own copy
source — shows the clip-source color, not blue: later same-specificity rules win the cascade.
They already are, in file order — do not reorder them above Step 2's rule.)

- [ ] **Step 6: Visual verification (no CSS unit-test harness exists in this repo — verify against the real running app per the UI-change verification requirement)**

Build and serve the web UI (check `package.json`/`README.md` for the existing dev-server command
— do not invent a new one), then with the `browser` tool:
1. Open a document with 3+ top-level keys.
2. Click one row (keyboard-cursor it), confirm full-row blue fill; hover a different row,
   confirm the same blue fill on the hovered row while the clicked row's cursor fill is
   unaffected.
3. Ctrl/Shift-click to lock-select 2 rows, confirm each shows only the thin leading bar (no
   fill), and if one of them is also the keyboard cursor, confirm it shows blue fill **and** the
   bar simultaneously.
4. Copy a row (`c`), confirm it fills purple; Cut a different row (`x` after Escape), confirm it
   fills green; confirm neither reads as blue.
5. Toggle dark mode, repeat steps 2-4 — confirm all four states stay visually distinct.

- [ ] **Step 7: Commit**

```bash
git add web/style.css
git commit -m "feat(web): unify cursor/hover/clip-source row fills, demote selection to a leading bar (ADR 0005 §2)"
```

---

### Task 3: Touch — add clip-source styling (currently absent), resting cursor fill, selection bar

**Files:**
- Modify: `web/touch/render.ts:57-65` (row class list), `web/touch/render.ts` `treeHTML`
  (compute clip class per row, mirroring `web/render.ts:198-201`).
- Modify: `web/touch/style.css:19` (light tokens), `:30` (dark tokens), `:117` (`.row.selected`),
  `:119` (dead `.row.cut` — delete), add new cursor/clip rules near `:117`.
- Test: `web/touch-clip-source.spec.mjs` (new file, follows `web/touch-render.spec.mjs`'s
  convention exactly).

**Interfaces:**
- Consumes: `SessionSnapshot.clipboard_paths: Path[]`, `SessionSnapshot.clipboard_cut: boolean`
  (`web/types.ts:177-179`, already exist, already used by desktop's `web/render.ts:198-201`).
- Produces: `rowHTML(r, idx, rows, pasteInto, clip)` gains a 5th parameter,
  `clip: "" | " clip-copy" | " clip-cut"` — same type shape as desktop's `renderRow`'s `clip`
  parameter (`web/render.ts:95`). `treeHTML(snap)`'s exported signature is unchanged (still takes
  only `snap`) since it computes `clip` internally per row, same as desktop's `renderTree`.

- [ ] **Step 1: Write the failing test**

Create `web/touch-clip-source.spec.mjs`:

```js
// Plain-Node test for touch/render.ts's clip-source styling (ADR 0005 §2): while the
// clipboard holds a copy or cut, the source row(s) must get the same `clip-copy`/
// `clip-cut` class desktop's web/render.ts already emits, keyed off
// SessionSnapshot.clipboard_paths/clipboard_cut. Follows touch-render.spec.mjs's
// convention: no test framework, just node:assert-style check() + esbuild bundling.
import path from "node:path";
import { fileURLToPath } from "node:url";
import * as esbuild from "esbuild";

const here = path.dirname(fileURLToPath(import.meta.url));

let failures = 0;
function check(name, cond, extra = "") {
  if (cond) {
    console.log(`  ✓ ${name}`);
  } else {
    console.log(`  ✗ ${name} ${extra}`);
    failures++;
  }
}

async function bundle(entry) {
  const result = await esbuild.build({
    entryPoints: [path.join(here, entry)],
    bundle: true,
    write: false,
    format: "esm",
    platform: "node",
    target: "es2022",
  });
  const code = result.outputFiles[0].text;
  const modUrl = "data:text/javascript;base64," + Buffer.from(code).toString("base64");
  return import(modUrl);
}

const { treeHTML } = await bundle("touch/render.ts");

function makeRow(overrides = {}) {
  return {
    path: [{ Key: "a" }],
    depth: 1,
    is_branch: true,
    key: "a",
    value: undefined,
    scalar_type: undefined,
    format: "Table",
    type_label: "table",
    child_count: 0,
    trailing_comment: undefined,
    read_only: false,
    violations: undefined,
    selected: false,
    is_cursor: false,
    ...overrides,
  };
}

function makeSnap(rows, overrides = {}) {
  return { rows, paste_slot: undefined, clipboard_paths: [], clipboard_cut: false, ...overrides };
}

console.log("-- treeHTML(): clip-copy/clip-cut key off clipboard_paths + clipboard_cut, per row --");
{
  const rowB = makeRow({ path: [{ Key: "b" }], key: "b" });
  const rowC = makeRow({ path: [{ Key: "c" }], key: "c" });
  const htmlPlain = treeHTML(makeSnap([rowB, rowC]));
  check("no clipboard: neither row gets a clip class", !htmlPlain.includes("clip-copy") && !htmlPlain.includes("clip-cut"));

  const htmlCopy = treeHTML(makeSnap([rowB, rowC], { clipboard_paths: [[{ Key: "b" }]], clipboard_cut: false }));
  const bDivCopy = htmlCopy.split("<div")[1];
  check("copy source row gets clip-copy", bDivCopy.includes("clip-copy"));
  const cDivCopy = htmlCopy.split("<div")[2];
  check("non-source sibling does not get clip-copy", !cDivCopy.includes("clip-copy"));

  const htmlCut = treeHTML(makeSnap([rowB, rowC], { clipboard_paths: [[{ Key: "b" }]], clipboard_cut: true }));
  const bDivCut = htmlCut.split("<div")[1];
  check("cut source row gets clip-cut, not clip-copy", bDivCut.includes("clip-cut") && !bDivCut.includes("clip-copy"));
}

console.log(failures === 0 ? "\nALL TOUCH CLIP-SOURCE CHECKS PASSED" : `\n${failures} FAILURES`);
process.exit(failures === 0 ? 0 : 1);
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `node web/touch-clip-source.spec.mjs`

Expected: FAIL — `makeSnap`'s new `clipboard_paths`/`clipboard_cut` fields are accepted but
`treeHTML` ignores them, so no row ever gets `clip-copy`/`clip-cut`.

- [ ] **Step 3: Implement — `web/touch/render.ts`**

Change the `rowHTML` signature (`web/touch/render.ts:50`) and its `cls` construction
(`:57-65`):

```ts
function rowHTML(
  r: ViewRow,
  idx: number,
  rows: ViewRow[],
  pasteInto: boolean,
  clip: "" | " clip-copy" | " clip-cut",
): string {
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
    (pasteInto ? " drop-into" : "") +
    clip;
```

Change `treeHTML` (`web/touch/render.ts:105-116`) to compute and pass `clip`, mirroring
`web/render.ts:198-201`:

```ts
export function treeHTML(snap: SessionSnapshot): string {
  const rows = snap.rows;
  const pasteIntoPath =
    snap.paste_slot && "Into" in snap.paste_slot ? JSON.stringify(snap.paste_slot.Into) : null;
  const clipKeys = new Set(snap.clipboard_paths.map((p) => JSON.stringify(p)));
  const clipCls: " clip-copy" | " clip-cut" = snap.clipboard_cut ? " clip-cut" : " clip-copy";
  return (
    rows
      .map((r, idx) =>
        r.path.length === 0
          ? ""
          : rowHTML(
              r,
              idx,
              rows,
              pasteIntoPath === JSON.stringify(r.path),
              clipKeys.has(JSON.stringify(r.path)) ? clipCls : "",
            ),
      )
      .join("") + '<div class="reorder-line"></div>'
  );
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `node web/touch-clip-source.spec.mjs`

Expected: PASS.

- [ ] **Step 5: Run the full web spec suite (this module is shared; nothing else may break)**

Run: `node web/run-tests.mjs`

Expected: PASS. If `web/touch-render.spec.mjs`'s `makeSnap` helper doesn't set
`clipboard_paths`/`clipboard_cut`, `treeHTML` now reads `snap.clipboard_paths.map(...)` on
`undefined` and throws — add `clipboard_paths: [], clipboard_cut: false` to that file's
`makeSnap` default return, matching this task's own `makeSnap` shape.

- [ ] **Step 6: Add the CSS — tokens**

In `web/touch/style.css`'s light `:root` block (the line already holding
`--sel:oklch(93% 0.05 250); --sel-edge:oklch(70% 0.12 250);`), append to the same line:

```css
--cursor-bg:oklch(93% 0.05 250); --cut-bg:oklch(93% 0.05 150); --copy-bg:oklch(93% 0.05 300);
```

In the dark `:root[data-theme="dark"]` block (the line holding
`--sel:oklch(34% 0.06 250); --sel-edge:oklch(62% 0.12 250);`), append:

```css
--cursor-bg:oklch(34% 0.06 250); --cut-bg:oklch(34% 0.06 150); --copy-bg:oklch(34% 0.06 300);
```

(Same values as Task 2's desktop tokens — both stylesheets already duplicate every other color
token verbatim; keep that existing convention rather than introducing a shared import.)

- [ ] **Step 7: Add the CSS — row rules**

Replace `web/touch/style.css:117` (`.row.selected > .row-main{background:var(--sel);...}`):

```css
.row.selected > .row-main::before{content:"";position:absolute;left:2px;top:4px;bottom:4px;width:3px;border-radius:2px;background:var(--sel-edge)}
.row.cursor > .row-main{background:var(--cursor-bg)}
.row.clip-cut > .row-main{background:var(--cut-bg)}
.row.clip-copy > .row-main{background:var(--copy-bg)}
```

(`.row-main` already has `position:relative` — `web/touch/style.css:110` — needed for the
`::before` bar to anchor inside it rather than being clipped behind `.row-main`'s own opaque
background. The plain `.row.cursor > .row-main` rule added here is intentionally lower
specificity than the existing `.app.paste-mode .row.cursor > .row-main` rule at
`web/touch/style.css:542-544` — that rule keeps winning during armed-paste-target highlighting,
which is a different state (PasteSlot targeting, ADR 0004) and is out of scope here.)

- [ ] **Step 8: Remove the dead `.row.cut` rule**

Delete `.row.cut > .row-main{opacity:.45}` (originally `web/touch/style.css:119`).

- [ ] **Step 9: Visual verification**

With the `browser` tool, load the touch UI (viewport sized for a touch layout per existing
project convention — check `README.md`/existing touch dev-server docs, don't invent a new one).
Tap a row (cursor), confirm blue fill now appears at rest (not just during paste mode). Long-
press/select 2 rows if the touch selection gesture is reachable at this stage of the model
(§1: touch has **no** locked-selection gesture today — if no such affordance exists yet, skip
this sub-check, it is expected, not a defect introduced by this task). Copy then Cut a row via
the FAB flow, confirm purple then green fills appear on the source row, matching desktop's hues
from Task 2.

- [ ] **Step 10: Commit**

```bash
git add web/touch/render.ts web/touch/style.css web/touch-clip-source.spec.mjs web/touch-render.spec.mjs
git commit -m "feat(touch): add clip-source row styling, resting cursor fill, selection bar (ADR 0005 §2)"
```

---

## Final integration check

- [ ] Run `cargo test -p confy-tui`, `node web/run-tests.mjs` one more time together — both must
  be green with all three tasks' changes present simultaneously (they touch disjoint files, so
  this is a formality, not expected to surface new conflicts).
- [ ] Re-read `ROW_STATE_MODEL.md` §8 Phase 1 checklist items and tick them off in that file to
  match this plan's completion.
