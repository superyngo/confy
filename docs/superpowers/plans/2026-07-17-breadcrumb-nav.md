# Breadcrumb Navigation Bar Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A VS Code-style breadcrumb bar in the confy web UI (desktop `index.html`, all three hosts incl. the VS Code webview) showing the cursor node's path; clicking any segment opens a **mini document tree** (lazy, expandable, pre-expanded along the cursor path, highlighted at the clicked segment) from which clicking any row **Reveals** it.

**Architecture:** One new core intent `RevealPath(Path)` implementing the glossary operation **Reveal** (expand all ancestors + set cursor; if a filter still hides the target, expansion sticks, cursor stays, status line reports it — `SetCursor` alone rejects non-visible paths, see `session.rs:200`). One new read-only query `Session::children_of(&Path) -> Vec<ChildView>` exposed through `confy-ffi` (mirrors the `kind_options` pattern) feeds the mini-tree's lazy expansion. One new web module `web/breadcrumb.ts` renders both the bar and the popup from each `SessionSnapshot`. No TUI changes, no VS Code extension/protocol changes (`host-vscode` CSS hides only `header.toolbar`; the breadcrumb sits below it). Touch UI (`web/touch/`) is explicitly out of scope.

**Tech Stack:** Rust (confy-core, confy-ffi/wasm-bindgen), TypeScript + esbuild (web), no new dependencies.

## Grilled decisions (2026-07-17, all confirmed by user)

- **Q1+Q2 (interaction):** VS Code parity — a bar segment click **opens the mini-tree popup** (no per-segment direct-jump, no ▾ chevron). Jumping happens inside the popup.
- **Q3 (content):** the mini-tree shows the **same node set as the main tree** — Comments and read-only/opaque nodes included and jumpable. No "navigable subset" concept.
- **Q4 (filter):** Reveal on a filter-hidden target → ancestors expand, cursor stays, **status line reports it** (core i18n key).
- **Q5 (term):** the operation is canonically named **Reveal** — already recorded in `CONTEXT.md` §Operations & projection (commit that edit in Task 5).
- **Q6 (glyphs):** VS Code-style text glyphs colored with the existing `--t-*` value-type hues (`{}` map-like, `[]` seq-like, `abc` string, `123` number, `tf` bool, `??` null, `@` datetime, `#` comment). No kind badges in nav surfaces.
- **Q7 (defaults):** popup expand state is **ephemeral** (each open resets to "cursor path expanded"); popup anchors under the clicked segment; Esc (capture-phase, swallowed — no filter peel) or outside pointerdown closes; bar sits below the filter row, hidden in Raw view; ⌂ root segment; pointer-only in v1 (no keyboard shortcut, TUI untouched); paste-mode jumps move the cursor = paste destination (same as tree clicks).

## Global Constraints

- `cargo clippy -- -D warnings` must be clean and `cargo fmt` run before every commit.
- `confy-core` stays filesystem-free (no `fs`/`process`/`env` — enforced by `tests/no_fs_gate.rs`).
- **esbuild deadlocks on the `/Volumes/Home` repo path** — the web bundle MUST be built from a scratchpad copy under `/tmp` and copied back (exact commands in Task 4).
- The working tree has **pre-existing uncommitted changes** unrelated to this work: `CHANGELOG.md`, `crates/confy-core/src/session/state.rs`, `crates/confy-tui/src/tui/mod.rs`, `crates/confy-tui/src/tui/ui.rs`, `editors/vscode/package.json`, `web/help-content.ts`, `editors/vscode/icon.png`. **Never `git add` these files** (CHANGELOG is handled specially in Task 5). Always `git add` explicit paths, never `git add -A`/`.`.
- `CONTEXT.md` already carries the new **Reveal** glossary entry (added during the grilling session) — it is part of this work and is committed in Task 5.
- Do all work on a local branch `feat/breadcrumb-nav` off `main`. **Never push** — the user merges manually.
- Do not drive the TUI or any long-lived interactive process; the user tests UIs manually.
- End every commit message with `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`.

---

### Task 1: Core `RevealPath` intent (the **Reveal** operation)

**Files:**
- Modify: `crates/confy-core/src/session/intent.rs` (after the `SetCursor` variant, line ~24)
- Modify: `crates/confy-core/src/session/dispatch.rs` (after the `Intent::SetCursor` arm, line ~68)
- Modify: `crates/confy-core/src/session/session.rs` (after `set_cursor`, line ~205)
- Modify: `i18n/en.json`, `i18n/zh-TW.json` (1 new `core.*` key each)
- Test: `crates/confy-core/tests/session_headless.rs` (append at end)

**Interfaces:**
- Consumes: existing `Session::set_cursor` visibility contract, `Session.expanded: HashSet<Path>`, `Session.tree.node_at(&Path)`, `Session.status: Option<String>`, `tr(self.lang, key)` (already used in this file, e.g. `session.rs:761`).
- Produces: `Intent::RevealPath(Path)` (serde external-tag `{ RevealPath: [...] }` on the wire) and `pub fn reveal_path(&mut self, path: Path)` — Task 3's smoke test and Task 4's web UI dispatch this.

- [ ] **Step 1: Write the failing tests**

Append to `crates/confy-core/tests/session_headless.rs`:

```rust
// ---- RevealPath (the "Reveal" operation — breadcrumb mini-tree jump) ----

#[test]
fn reveal_path_expands_ancestors_and_sets_cursor() {
    let mut s = toml_session("[a]\n[a.b]\nx = 1\n");
    // Everything starts collapsed: only root + `a` are visible.
    let target = vec![
        Seg::Key("a".into()),
        Seg::Key("b".into()),
        Seg::Key("x".into()),
    ];
    s.dispatch(Intent::RevealPath(target.clone()));
    let rows = s.visible_rows();
    let cursor_row = rows.iter().find(|r| r.is_cursor).unwrap();
    assert_eq!(cursor_row.path, target);
}

#[test]
fn reveal_path_ignores_unknown_path() {
    let mut s = toml_session("a = 1\n");
    let before = s.visible_rows().len();
    let snap = s.dispatch(Intent::RevealPath(vec![Seg::Key("nope".into())]));
    assert_eq!(s.visible_rows().len(), before, "no expansion happened");
    assert!(snap.status.is_none(), "unknown path is a silent no-op");
    let rows = s.visible_rows();
    let cursor_row = rows.iter().find(|r| r.is_cursor).unwrap();
    assert_eq!(cursor_row.key, "", "cursor stays on root");
}

#[test]
fn reveal_path_hidden_by_filter_expands_and_reports() {
    let mut s = toml_session("port = 8080\n[a]\nx = 1\n");
    s.dispatch(Intent::SetFilter("port".into()));
    // `a.x` exists but the filter hides it: expansion sticks, cursor doesn't
    // move onto it, and the status line says so (grilled decision Q4/C).
    let snap = s.dispatch(Intent::RevealPath(vec![
        Seg::Key("a".into()),
        Seg::Key("x".into()),
    ]));
    let rows = s.visible_rows();
    let cursor_row = rows.iter().find(|r| r.is_cursor).unwrap();
    assert_ne!(cursor_row.key, "x");
    assert!(
        snap.status.is_some(),
        "hidden-by-filter must report on the status line"
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p confy-core --test session_headless reveal_path`
Expected: compile error — `no variant named RevealPath`.

- [ ] **Step 3: Add the intent variant**

In `crates/confy-core/src/session/intent.rs`, directly after the `SetCursor(crate::model::node::Path),` variant:

```rust
    /// **Reveal** (CONTEXT.md §Operations): expand every ancestor of `path`
    /// and place the cursor on it (Web UI breadcrumb mini-tree jump). No-op if
    /// the path doesn't exist; if an active filter still hides the row, the
    /// expansion sticks, the cursor stays put, and the status line reports it.
    RevealPath(crate::model::node::Path),
```

- [ ] **Step 4: Route it in dispatch**

In `crates/confy-core/src/session/dispatch.rs`, directly after the `Intent::SetCursor(path) => self.set_cursor(path),` arm:

```rust
            Intent::RevealPath(path) => self.reveal_path(path),
```

- [ ] **Step 5: Add the i18n key**

In `i18n/en.json`, after the `"core.error.generic"` line (line ~9):

```json
  "core.reveal.hidden-by-filter": "target is hidden by the active filter",
```

In `i18n/zh-TW.json`, in the same `core.*` block:

```json
  "core.reveal.hidden-by-filter": "目標被目前的篩選條件隱藏",
```

(Mind trailing commas — both files are plain JSON.)

- [ ] **Step 6: Implement `reveal_path`**

In `crates/confy-core/src/session/session.rs`, directly after the `set_cursor` method (ends ~line 205):

```rust
    /// **Reveal** (CONTEXT.md §Operations): expand every ancestor prefix of
    /// `path`, then place the cursor on it. Unknown paths are ignored; if an
    /// active filter still hides the row, the expansion sticks, the cursor
    /// stays put, and the status line says so.
    pub fn reveal_path(&mut self, path: Path) {
        if self.tree.node_at(&path).is_none() {
            return;
        }
        for i in 0..path.len() {
            self.expanded.insert(path[..i].to_vec());
        }
        let visible = self.visible_nodes().iter().any(|r| r.node.path == path);
        if visible {
            self.cursor = path;
        } else {
            self.status = Some(tr(self.lang, "core.reveal.hidden-by-filter").to_string());
        }
    }
```

(`Path`, `self.expanded`, `self.tree.node_at`, `self.visible_nodes`, `self.status`, and `tr` are all already in scope/used in this file — no new imports.)

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo test -p confy-core --test session_headless reveal_path`
Expected: 3 passed.

- [ ] **Step 8: Full check + commit**

```bash
cargo test -p confy-core && cargo clippy -- -D warnings && cargo fmt
git checkout -b feat/breadcrumb-nav
git add crates/confy-core/src/session/intent.rs crates/confy-core/src/session/dispatch.rs crates/confy-core/src/session/session.rs crates/confy-core/tests/session_headless.rs i18n/en.json i18n/zh-TW.json
git commit -m "feat(core): RevealPath intent — the Reveal operation (expand ancestors + set cursor)

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 2: Core `children_of` query + ffi `children()`

**Files:**
- Modify: `crates/confy-core/src/session/view.rs` (add `ChildView` near `ViewRow`, line ~10)
- Modify: `crates/confy-core/src/session/session.rs` (add `children_of` after `reveal_path`)
- Modify: `crates/confy-core/src/session/mod.rs` (extend the `pub use view::{…}` list, line ~29)
- Modify: `crates/confy-ffi/src/lib.rs` (new method after `kind_options`, line ~105)
- Test: `crates/confy-core/tests/session_headless.rs` (append)

**Interfaces:**
- Consumes: `Session.tree.node_at(&Path)`, `Node { key, path, kind, children }`, `Node::is_branch()`, free fn `node_type_label(&NodeKind)` (defined in `session.rs:2871`).
- Produces: `pub struct ChildView { key: String, path: Path, type_label: String, is_branch: bool }` (serde both ways), `pub fn children_of(&self, path: &Path) -> Vec<ChildView>` on `Session`, and wasm method `ConfySession::children(path: JsValue) -> Result<JsValue, JsValue>`. Task 3 smoke-tests `children`; Task 4's `web/confy.ts` calls it (mini-tree lazy expansion + bar glyph lookup).

- [ ] **Step 1: Write the failing test**

Append to `crates/confy-core/tests/session_headless.rs`:

```rust
// ---- children_of (breadcrumb mini-tree lazy query) ----

#[test]
fn children_of_lists_children_of_a_collapsed_branch() {
    let s = toml_session("[a]\nx = 1\ny = 2\n");
    // `a` is collapsed — children_of must not depend on expansion state.
    let kids = s.children_of(&vec![Seg::Key("a".into())]);
    assert_eq!(kids.len(), 2);
    assert_eq!(kids[0].key, "x");
    assert_eq!(kids[0].type_label, "integer");
    assert!(!kids[0].is_branch);
    assert_eq!(
        kids[1].path,
        vec![Seg::Key("a".into()), Seg::Key("y".into())]
    );
    // Unknown path → empty, never a panic.
    assert!(s.children_of(&vec![Seg::Key("nope".into())]).is_empty());
}

#[test]
fn children_of_includes_comments() {
    // Grilled decision Q3/A: the mini-tree shows the same node set as the main
    // tree — a Comment is a first-class child.
    let s = toml_session("# note\na = 1\n");
    let kids = s.children_of(&Vec::new());
    assert_eq!(kids.len(), 2);
    assert_eq!(kids[0].type_label, "comment");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p confy-core --test session_headless children_of`
Expected: compile error — `no method named children_of`.

- [ ] **Step 3: Add `ChildView` to view.rs**

In `crates/confy-core/src/session/view.rs`, directly above `pub struct ViewRow` (both `Path` and the serde derives are already imported in this file):

```rust
/// One immediate child of a node — the Web UI breadcrumb mini-tree row
/// (returned by `Session::children_of`, exposed as ffi `children(path)`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChildView {
    pub key: String,
    pub path: Path,
    /// Core type label ("table"/"array"/"string"/"comment"/…), same vocabulary
    /// as `ViewRow::type_label`.
    pub type_label: String,
    pub is_branch: bool,
}
```

- [ ] **Step 4: Implement `children_of`**

In `crates/confy-core/src/session/session.rs`, directly after `reveal_path` (Task 1):

```rust
    /// Immediate children of the node at `path`, independent of expansion
    /// state — the Web UI breadcrumb mini-tree's lazy query (read-only,
    /// mirrors the `kind_options` pattern). Unknown paths return an empty list.
    pub fn children_of(&self, path: &Path) -> Vec<ChildView> {
        let Some(node) = self.tree.node_at(path) else {
            return Vec::new();
        };
        node.children
            .iter()
            .map(|c| ChildView {
                key: c.key.clone(),
                path: c.path.clone(),
                type_label: node_type_label(&c.kind),
                is_branch: c.is_branch(),
            })
            .collect()
    }
```

Add `ChildView` to the existing `use` of view types at the top of `session.rs` (the file already imports from the view module — extend that list).

- [ ] **Step 5: Re-export from mod.rs**

In `crates/confy-core/src/session/mod.rs`, add `ChildView` to the existing `pub use view::{…}` list (line ~29), keeping the list's existing ordering style.

- [ ] **Step 6: Run test to verify it passes**

Run: `cargo test -p confy-core --test session_headless children_of`
Expected: 2 passed.

- [ ] **Step 7: Add the ffi method**

In `crates/confy-ffi/src/lib.rs`, directly after the `kind_options` method (line ~105), inside the same `#[wasm_bindgen] impl ConfySession` block:

```rust
    /// Immediate children of the node at `path` (breadcrumb mini-tree), as
    /// `ChildView[]` — independent of expansion state.
    pub fn children(&self, path: JsValue) -> Result<JsValue, JsValue> {
        let path: Path = from_value(path).map_err(js_serde_error)?;
        to_value(&self.session.children_of(&path)).map_err(js_serde_error)
    }
```

(No new imports — `Path`, `from_value`, `to_value`, `js_serde_error` are already used in this file.)

- [ ] **Step 8: Check ffi compiles + commit**

```bash
cargo test -p confy-core && cargo check -p confy-ffi && cargo clippy -- -D warnings && cargo fmt
git add crates/confy-core/src/session/view.rs crates/confy-core/src/session/session.rs crates/confy-core/src/session/mod.rs crates/confy-core/tests/session_headless.rs crates/confy-ffi/src/lib.rs
git commit -m "feat(core+ffi): children_of query for the breadcrumb mini-tree

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 3: Wasm rebuild + functional smoke checks

**Files:**
- Modify: `crates/confy-ffi/functional_smoke.mjs` (insert before the final `console.log(failures === 0 …)` line)

**Interfaces:**
- Consumes: Task 1's `{ RevealPath: Path }` wire shape (+ hidden-by-filter status), Task 2's `children(path)` returning `[{ key, path, type_label, is_branch }]`.
- Produces: a rebuilt `crates/confy-ffi/pkg/` (Task 4's web build copies it into the bundle).

- [ ] **Step 1: Add the smoke checks**

In `crates/confy-ffi/functional_smoke.mjs`, insert immediately **before** the final `console.log(failures === 0 ? …)` line:

```js
// ---- 25. Breadcrumb: RevealPath (Reveal) + children(path) ----
{
  const sb = new ConfySession('[a]\n[a.b]\nx = 1\nport = 8080\n', "toml");
  const kids = sb.children([{ Key: "a" }]);
  check("children() lists a collapsed branch's child", kids.length === 1 && kids[0].key === "b", JSON.stringify(kids));
  check("children() child carries path + type_label + is_branch",
    kids[0].type_label === "table" && kids[0].is_branch === true && kids[0].path.length === 2,
    JSON.stringify(kids[0]));
  check("children() empty on unknown path", sb.children([{ Key: "zzz" }]).length === 0);
  let snb = sb.dispatch(tuple("RevealPath", [{ Key: "a" }, { Key: "b" }, { Key: "x" }]));
  const curRow = snb.rows.find((r) => r.is_cursor);
  check("RevealPath expands ancestors and sets cursor", curRow && curRow.key === "x", JSON.stringify(snb.cursor));
  snb = sb.dispatch(tuple("RevealPath", [{ Key: "zzz" }]));
  check("RevealPath no-ops on an unknown path", snb.rows.find((r) => r.is_cursor).key === "x");
  sb.dispatch(tuple("SetFilter", "port"));
  snb = sb.dispatch(tuple("RevealPath", [{ Key: "a" }, { Key: "b" }, { Key: "x" }]));
  check("RevealPath hidden-by-filter reports on status", typeof snb.status === "string" && snb.status.length > 0, snb.status);
  sb.free();
}
```

- [ ] **Step 2: Rebuild the wasm package**

```bash
cd /Volumes/Home/Users/wen/repos/confy/crates/confy-ffi && wasm-pack build --target web
```

Expected: exits 0, refreshes `pkg/`.

- [ ] **Step 3: Run the smoke suite**

```bash
cd /Volumes/Home/Users/wen/repos/confy/crates/confy-ffi && node functional_smoke.mjs
```

Expected: last line `ALL FUNCTIONAL CHECKS PASSED` (exit 0), including the 6 new `✓` lines.

- [ ] **Step 4: Commit**

```bash
git add crates/confy-ffi/functional_smoke.mjs
git commit -m "test(ffi): smoke-check RevealPath + children()

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 4: Web breadcrumb bar + mini-tree popup

**Files:**
- Modify: `web/types.ts` (Intent union line ~165 + new `ChildView` interface after `ViewRow`)
- Modify: `web/confy.ts` (new `children()` on `Session`, after `kindOptions`)
- Create: `web/breadcrumb.ts`
- Modify: `web/index.html` (insert `<nav id="crumbs">` after the filter block's closing `</div>`, line ~114, before `<!-- ===== main ===== -->`)
- Modify: `web/style.css` (append at end of file — the app-only appendix)
- Modify: `web/ui.ts` (element ref + `render()` hook + dismiss wiring)
- Modify: `i18n/en.json`, `i18n/zh-TW.json` (1 new `web.*` key each)

**Interfaces:**
- Consumes: Task 2's ffi `children(path)`, Task 1's `RevealPath`; existing `escapeHtml` (from `web/escape.ts` — check whether `panel.ts` imports it from `./escape.js` or `./render.js` and match), `pathEq` from `web/path-utils.ts`, `t()` from `web/i18n.ts`, `send()`/`render()`/`rawView` in `ui.ts`, snapshot field `cursor: Path`, the existing `.t-*` value-hue CSS classes.
- Produces: `renderCrumbs(bar, snap, deps)` + `wireCrumbDismiss()` from `web/breadcrumb.ts`; `Session.children(path): ChildView[]` in `confy.ts`.

- [ ] **Step 1: Extend types.ts**

In `web/types.ts`, in the `Intent` union after the `| { SetCursor: Path }` line:

```ts
  | { RevealPath: Path }
```

And after the `ViewRow` interface:

```ts
// ---- Breadcrumb children query (session::view::ChildView, ffi `children`) ----
export interface ChildView {
  key: string;
  path: Path;
  type_label: string;
  is_branch: boolean;
}
```

- [ ] **Step 2: Extend confy.ts**

Add `ChildView` to the existing type import from `./types.js`, then after the `kindOptions` method:

```ts
  /** Immediate children of the node at `path` (breadcrumb mini-tree). */
  children(path: Path): ChildView[] {
    return this.raw.children(path) as ChildView[];
  }
```

- [ ] **Step 3: Add the i18n key**

In `i18n/en.json`, after the `"web.search.placeholder"` line (line ~99):

```json
  "web.crumbs.root.title": "Document root",
```

In `i18n/zh-TW.json`, after its `"web.search.placeholder"` line (line ~100):

```json
  "web.crumbs.root.title": "文件根節點",
```

- [ ] **Step 4: Create web/breadcrumb.ts**

```ts
// Breadcrumb bar + mini-tree picker — the VS Code-style symbol path for the
// cursor node, rendered between the filter row and the tree (visible in every
// host, including the VS Code webview, whose native breadcrumb shows only the
// file segment for custom editors).
//
// Bar anatomy: ⌂ root, then one segment per cursor-path `Seg` (`Key` → name,
// `Index` → `[i]`), each with a type glyph. Clicking ANY segment opens the
// mini-tree popup (VS Code parity — grilled decision Q1+Q2/B): a lazy mini
// document tree fed by the ffi `children(path)` query, pre-expanded along the
// cursor path, highlighted at the clicked segment; row carets expand/collapse
// freely, clicking a row body Reveals it (`RevealPath` — expand ancestors +
// set cursor; CONTEXT.md §Operations "Reveal") and closes the popup. The
// popup's expand state is ephemeral — every open resets (Q7.1). The mini-tree
// shows the same node set as the main tree — comments and read-only nodes
// included (Q3/A). Pure render-from-snapshot; the only module state is the
// open popup + its ephemeral expand set, which any re-render, outside
// pointerdown, or a capture-phase Escape closes (Escape is swallowed so it
// doesn't also peel filter state — panel.ts stopPropagation precedent).
import type { ChildView, Path, Seg, SessionSnapshot } from "./types.js";
import { escapeHtml } from "./escape.js";
import { pathEq } from "./path-utils.js";
import { t } from "./i18n.js";

export interface CrumbDeps {
  /** Immediate children of the node at `path` (ffi `children`), lazy. */
  children(path: Path): ChildView[];
  /** Dispatch `RevealPath` (the Reveal operation). */
  jump(path: Path): void;
}

// VS Code-style text glyph + value-type hue token per core type_label (Q6/A).
const GLYPHS: Record<string, [glyph: string, hue: string]> = {
  table: ["{}", "branch"],
  inline: ["{}", "branch"],
  array: ["[]", "branch"],
  "array-of-tables": ["[]", "branch"],
  string: ["abc", "string"],
  integer: ["123", "number"],
  float: ["123", "number"],
  bool: ["tf", "bool"],
  null: ["??", "null"],
  offsetdatetime: ["@", "date"],
  localdatetime: ["@", "date"],
  localdate: ["@", "date"],
  localtime: ["@", "date"],
  comment: ["#", "null"],
};

function glyphHTML(typeLabel: string): string {
  const [g, hue] = GLYPHS[typeLabel] ?? ["··", "null"];
  return `<span class="crumb-glyph mono t-${hue}">${escapeHtml(g)}</span>`;
}

function segLabel(seg: Seg): string {
  return "Key" in seg ? escapeHtml(seg.Key) : `[${seg.Index}]`;
}

// ---- popup state (ephemeral per open) ----
let openMenu: HTMLElement | null = null;
let treeExpanded = new Set<string>(); // JSON.stringify(path) keys

function closeMenu(): void {
  openMenu?.remove();
  openMenu = null;
}

// ---- bar ----
export function renderCrumbs(bar: HTMLElement, snap: SessionSnapshot, deps: CrumbDeps): void {
  closeMenu();
  const cur = snap.cursor;
  const parts: string[] = [
    `<button class="crumb${cur.length === 0 ? " current" : ""}" data-i="0" title="${t("web.crumbs.root.title")}">⌂</button>`,
  ];
  for (let i = 0; i < cur.length; i++) {
    // A segment's own type comes from its parent's children list (the bar has
    // only `Seg`s; ChildView carries the type). Depth is small — one lazy
    // query per level per render is fine.
    const self = cur.slice(0, i + 1);
    const info = deps.children(cur.slice(0, i)).find((k) => pathEq(k.path, self));
    parts.push(`<span class="crumb-sep">›</span>`);
    parts.push(
      `<button class="crumb${i === cur.length - 1 ? " current" : ""}" data-i="${i + 1}">` +
        (info ? glyphHTML(info.type_label) : "") +
        `<span>${segLabel(cur[i])}</span></button>`,
    );
  }
  bar.innerHTML = parts.join("");
  bar.querySelectorAll<HTMLElement>("button.crumb").forEach((b) =>
    b.addEventListener("click", (ev) => {
      ev.stopPropagation();
      openTree(b, snap, deps, cur.slice(0, Number(b.dataset.i)));
    }),
  );
  // Keep the tail (current node) in view when a deep path overflows.
  bar.scrollLeft = bar.scrollWidth;
}

// ---- mini-tree popup ----
function openTree(anchor: HTMLElement, snap: SessionSnapshot, deps: CrumbDeps, highlight: Path): void {
  if (openMenu?.dataset.i === anchor.dataset.i) {
    closeMenu(); // second click on the same segment toggles the popup off
    return;
  }
  closeMenu();
  // Ephemeral expand state: reset to "expanded along the cursor path" (Q7.1).
  // Every prefix INCLUDING the cursor node itself (expanding a leaf is a
  // harmless no-op — it has no children), plus the clicked segment.
  treeExpanded = new Set<string>();
  for (let i = 0; i <= snap.cursor.length; i++) {
    treeExpanded.add(JSON.stringify(snap.cursor.slice(0, i)));
  }
  treeExpanded.add(JSON.stringify(highlight));

  const menu = document.createElement("div");
  menu.className = "crumb-menu";
  menu.dataset.i = anchor.dataset.i!;
  renderTreeRows(menu, deps, highlight);
  menu.addEventListener("click", (ev) => {
    const rowEl = (ev.target as Element).closest<HTMLElement>(".crumb-row");
    if (!rowEl) return;
    if ((ev.target as Element).closest(".crumb-caret:not(.none)")) {
      const key = rowEl.dataset.path!;
      if (treeExpanded.has(key)) treeExpanded.delete(key);
      else treeExpanded.add(key);
      const scroll = menu.scrollTop;
      renderTreeRows(menu, deps, highlight);
      menu.scrollTop = scroll;
      return;
    }
    const path = JSON.parse(rowEl.dataset.path!) as Path;
    closeMenu();
    deps.jump(path);
  });
  document.body.appendChild(menu);
  const r = anchor.getBoundingClientRect();
  menu.style.left = `${Math.max(8, Math.min(r.left, window.innerWidth - menu.offsetWidth - 8))}px`;
  menu.style.top = `${r.bottom + 4}px`;
  openMenu = menu;
  menu.querySelector(".current")?.scrollIntoView({ block: "nearest" });
}

function renderTreeRows(menu: HTMLElement, deps: CrumbDeps, highlight: Path): void {
  const rows: string[] = [
    // Root row first — jumpable like the main tree's root row.
    `<div class="crumb-row${highlight.length === 0 ? " current" : ""}" data-path="[]" style="--d:0">` +
      `<span class="crumb-caret none"></span>` +
      `<span class="crumb-glyph mono t-branch">⌂</span>` +
      `<span class="crumb-label">${t("web.crumbs.root.title")}</span></div>`,
  ];
  const walk = (path: Path, depth: number): void => {
    for (const k of deps.children(path)) {
      const key = JSON.stringify(k.path);
      const open = treeExpanded.has(key);
      const last = k.path[k.path.length - 1];
      // Keyless children (array elements, comments) fall back to `[i]`.
      const label = k.key !== "" ? escapeHtml(k.key) : last ? segLabel(last) : "…";
      rows.push(
        `<div class="crumb-row${pathEq(k.path, highlight) ? " current" : ""}" data-path="${escapeHtml(key)}" style="--d:${depth}">` +
          (k.is_branch
            ? `<button class="crumb-caret" aria-expanded="${open}">${open ? "⌄" : "›"}</button>`
            : `<span class="crumb-caret none"></span>`) +
          glyphHTML(k.type_label) +
          `<span class="crumb-label">${label}</span></div>`,
      );
      if (k.is_branch && open) walk(k.path, depth + 1);
    }
  };
  walk([], 1);
  menu.innerHTML = rows.join("");
}

// ---- dismissal (wired once) ----
let dismissWired = false;
export function wireCrumbDismiss(): void {
  if (dismissWired) return;
  dismissWired = true;
  document.addEventListener("pointerdown", (ev) => {
    if (
      openMenu &&
      !openMenu.contains(ev.target as Node) &&
      !(ev.target as Element).closest(".crumbs")
    ) {
      closeMenu();
    }
  });
  // Capture phase so an open popup swallows Escape before the app's global
  // peel handler (same precedent as panel.ts's stopPropagation).
  document.addEventListener(
    "keydown",
    (ev) => {
      if (ev.key === "Escape" && openMenu) {
        closeMenu();
        ev.stopPropagation();
        ev.preventDefault();
      }
    },
    true,
  );
}
```

Note: if `web/escape.ts` does not export `escapeHtml` directly, import it from `./render.js` instead (`render.ts:18` re-exports it) — check which import `panel.ts` uses and match it. Likewise verify the `.t-branch`/`.t-string`/… classes exist in `style.css` (render.ts's `typeClass` emits them); if the class names differ, match render.ts's.

- [ ] **Step 5: Mount point in index.html**

In `web/index.html`, after the filter block's closing `</div>` (line ~114, the one right after `#viewTabs`) and before `<!-- ===== main ===== -->`:

```html
    <!-- ===== breadcrumb (cursor path — see web/breadcrumb.ts) ===== -->
    <nav class="crumbs" id="crumbs" aria-label="breadcrumb"></nav>
```

- [ ] **Step 6: Styles**

Append at the end of `web/style.css`:

```css
/* ===== Breadcrumb bar + mini-tree (cursor path, VS Code-style — web/breadcrumb.ts) ===== */
.crumbs {
  display: flex; align-items: center; gap: 1px;
  padding: 3px 14px; font-size: 12px; color: var(--muted);
  border-bottom: 1px solid var(--border);
  overflow-x: auto; white-space: nowrap; scrollbar-width: none;
  user-select: none;
}
.crumbs::-webkit-scrollbar { display: none; }
.crumbs .crumb {
  display: inline-flex; align-items: center; gap: 4px;
  border: 0; background: none; color: inherit; font: inherit;
  padding: 2px 4px; border-radius: 4px; cursor: pointer;
}
.crumbs .crumb:hover { background: var(--accent-soft); color: var(--fg); }
.crumbs .crumb.current { color: var(--fg); font-weight: 600; }
.crumb-sep { color: var(--faint); padding: 0 1px; }
.crumb-glyph { font-size: 10px; min-width: 20px; text-align: center; }
.crumb-menu {
  position: fixed; z-index: 60; min-width: 200px; max-width: 340px;
  max-height: 50vh; overflow: auto;
  background: var(--surface); border: 1px solid var(--border-strong);
  border-radius: 8px; box-shadow: var(--shadow); padding: 4px;
  font-size: 12px;
}
.crumb-row {
  display: flex; align-items: center; gap: 6px;
  padding: 4px 8px 4px calc(8px + var(--d) * 14px);
  border-radius: 5px; cursor: pointer; color: var(--fg);
}
.crumb-row:hover { background: var(--accent-soft); }
.crumb-row.current { background: var(--sel); }
.crumb-caret {
  border: 0; background: none; color: var(--faint); font: inherit;
  width: 14px; padding: 0; cursor: pointer; text-align: center;
}
.crumb-caret:hover { color: var(--fg); }
.crumb-caret.none { display: inline-block; width: 14px; }
.crumb-label {
  overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
}
```

(If the popup renders underneath the detail aside or a modal, adjust `z-index` against the existing layers in style.css — pick one lower than the modal scrim.)

- [ ] **Step 7: Wire into ui.ts**

Add the import near the other module imports:

```ts
import { renderCrumbs, wireCrumbDismiss } from "./breadcrumb.js";
```

Add the element ref near the other `$(…)` element consts (e.g. next to `fmtPill`), and wire dismissal once at startup:

```ts
const crumbsEl = $("crumbs");
wireCrumbDismiss();
```

In `render()` (line ~285), directly after the `renderRawOrTree();` call:

```ts
  crumbsEl.classList.toggle("hidden", rawView);
  if (!rawView) {
    renderCrumbs(crumbsEl, snap, {
      children: (p) => session!.children(p),
      jump: (p) => send({ RevealPath: p }),
    });
  }
```

(The `RevealPath` dispatch triggers a full `render()`; `render.ts:212` already `scrollIntoView`s the cursor row, so the jumped-to row scrolls into view with no extra code. In paste mode the jump moves the cursor = paste destination, same as a tree click — no special casing.)

- [ ] **Step 8: Typecheck + bundle (scratchpad — esbuild hangs on /Volumes/Home)**

```bash
SCRATCH=$(mktemp -d /tmp/confy-web.XXXXXX)
mkdir -p "$SCRATCH/crates/confy-ffi"
rsync -a --exclude node_modules --exclude dist /Volumes/Home/Users/wen/repos/confy/web/ "$SCRATCH/web/"
rsync -a /Volumes/Home/Users/wen/repos/confy/i18n/ "$SCRATCH/i18n/"
rsync -a /Volumes/Home/Users/wen/repos/confy/crates/confy-ffi/pkg/ "$SCRATCH/crates/confy-ffi/pkg/"
cp /Volumes/Home/Users/wen/repos/confy/Cargo.toml "$SCRATCH/Cargo.toml"
cd "$SCRATCH/web" && npm install && npm run typecheck && node build.mjs
```

Expected: `tsc --noEmit` clean; last line `built: ui.js + touch/app.js + pkg/`.

- [ ] **Step 9: Copy build outputs back**

```bash
cp "$SCRATCH/web/ui.js" "$SCRATCH/web/ui.js.map" /Volumes/Home/Users/wen/repos/confy/web/
cp "$SCRATCH/web/touch/app.js" "$SCRATCH/web/touch/app.js.map" /Volumes/Home/Users/wen/repos/confy/web/touch/
rsync -a --delete "$SCRATCH/web/pkg/" /Volumes/Home/Users/wen/repos/confy/web/pkg/
```

- [ ] **Step 10: Hand to the user for manual verification**

Do NOT drive the browser or a dev server yourself. Report: run `node serve.mjs` in `web/` and check (1) the bar tracks the cursor with glyphs per segment, (2) clicking any segment opens the mini-tree pre-expanded along the cursor path with the clicked node highlighted and scrolled into view, (3) carets expand/collapse lazily (deep + wide docs stay snappy), (4) clicking a row — including a comment row — Reveals it and closes the popup, (5) with an active filter, Revealing a hidden node keeps the filter, keeps the cursor, and shows the status message, (6) Esc / outside click closes the popup without peeling filter state, (7) Raw view hides the bar, (8) dark + light themes, (9) paste mode: a jump moves the paste destination.

- [ ] **Step 11: Commit**

```bash
cd /Volumes/Home/Users/wen/repos/confy
git add web/types.ts web/confy.ts web/breadcrumb.ts web/index.html web/style.css web/ui.ts i18n/en.json i18n/zh-TW.json web/ui.js web/ui.js.map web/touch/app.js web/touch/app.js.map
git commit -m "feat(web): breadcrumb bar + mini-tree picker (Reveal navigation)

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

(If `web/ui.js` / `web/pkg` are gitignored, drop them from the `git add` list — stage only what `git status` shows as tracked/untracked source.)

---

### Task 5: Docs + changelog + glossary commit

**Files:**
- Modify: `WEBUI.md` (new subsection under `## Web UI architecture`, and add `children` to the `## FFI API surface` method list at line ~46)
- Modify: `CLAUDE.md` (module map: one line for `breadcrumb.ts` between the `prompt.ts` and `ui.ts` entries)
- Commit: `CONTEXT.md` (the **Reveal** glossary entry was already added during the grilling session — commit it here, no further edits)
- Modify: `CHANGELOG.md` (append an `Unreleased Update` entry — **do not commit this file**, it carries unrelated pre-existing edits)

**Interfaces:**
- Consumes: everything shipped in Tasks 1–4 (describe, don't change).
- Produces: nothing downstream.

- [ ] **Step 1: WEBUI.md**

Under `## FFI API surface`, add to the `ConfySession` method list:

```
- `children(path)` — immediate children of the node at `path` as `ChildView[]` (`{ key, path, type_label, is_branch }`), independent of expansion state; feeds the breadcrumb mini-tree's lazy expansion.
```

Under `## Web UI architecture`, add a subsection (place it after whichever subsection documents the toolbar/filter row):

```markdown
### Breadcrumb bar + mini-tree (`web/breadcrumb.ts`)

A VS Code-style symbol path for the cursor node, in the `#crumbs` nav between
the filter row and the tree (all hosts — in the VS Code webview it supplies the
symbol segments the workbench's native breadcrumb can't show for custom
editors). One glyph-tagged segment per cursor-path `Seg` (⌂ root first; `Index`
segs render `[i]`; glyphs are VS Code-style text tags colored by the `--t-*`
value hues). Clicking **any** segment opens the mini-tree popup: a lazy mini
document tree fed by the ffi `children(path)` query, pre-expanded along the
cursor path, highlighted at the clicked segment; carets expand/collapse freely
(expand state is ephemeral per open), and clicking a row **Reveals** it —
`RevealPath` expands every ancestor and sets the cursor (plain `SetCursor`
rejects non-visible paths), after which the tree's existing cursor
`scrollIntoView` brings the row on-screen. If an active filter still hides the
target, the expansion sticks, the cursor stays, and the status line reports it.
The mini-tree shows the same node set as the main tree (comments and read-only
nodes included and jumpable). The popup is the module's only state — re-render,
outside pointerdown, or a capture-phase Escape closes it (the Escape is
swallowed so it doesn't also peel filter state). Hidden in Raw view. Not in the
touch UI (deliberate — touch is sheet-driven with a weak cursor concept).
```

- [ ] **Step 2: CLAUDE.md module map line**

In the `web/` block of the module map, between the `prompt.ts` and `ui.ts` entries:

```
  breadcrumb.ts  VS Code-style breadcrumb bar + mini-tree picker: any segment click
                 opens a lazy mini document tree (ffi children(path)), row click →
                 RevealPath ("Reveal": expand ancestors + set cursor; filter-hidden
                 targets keep cursor + report on status); popup state is ephemeral
```

- [ ] **Step 3: CHANGELOG.md entry (leave uncommitted)**

Append under the unreleased section, matching the file's existing entry style:

```markdown
### Unreleased Update — 2026-07-17
- feat(web): breadcrumb bar + mini-tree picker below the filter row — segment
  click opens a lazy mini document tree (new ffi `children(path)` query),
  row click Reveals the node via the new core `RevealPath` intent (expands
  ancestors + sets cursor; filter-hidden targets keep the cursor and report on
  the status line). All web hosts (browser / Tauri / VS Code webview); touch UI
  excluded. New glossary term: Reveal (CONTEXT.md §Operations).
```

- [ ] **Step 4: Commit docs + glossary (not CHANGELOG)**

```bash
git add WEBUI.md CLAUDE.md CONTEXT.md
git commit -m "docs: breadcrumb mini-tree (WEBUI.md, CLAUDE.md) + Reveal glossary term

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

Flag in the final report: `CHANGELOG.md` was appended but left uncommitted because it already carried unrelated uncommitted edits.

---

## Self-review notes

- Spec coverage: bar 渲染+glyph (T4 §4–7 / Q6)、segment 點擊=開迷你樹 (Q1+Q2/B, T4 §4)、迷你樹 lazy 展開+高亮+跳選 (T2 + T4)、filter 遮蔽提示 (Q4/C, T1 §5–6)、與主樹同節點集合 (Q3/A, T2 test 2)、glossary "Reveal" (Q5, CONTEXT.md done + T5 commit)、VS Code host 可見性 (架構備註,零 extension 改動)、i18n (T1 §5, T4 §3)、docs/changelog (T5)。
- Type consistency: `ChildView { key, path, type_label, is_branch }` identical in view.rs / types.ts / smoke checks; `RevealPath(Path)` wire shape `{ RevealPath: [...] }` used consistently; `renderCrumbs`/`wireCrumbDismiss` names match between breadcrumb.ts and ui.ts.
- Known judgment calls an implementer may hit: `escapeHtml` import source (noted in T4 §4), `.t-*` hue class names (noted in T4 §4), `.crumb-menu` z-index vs. existing layers (noted in T4 §6), gitignore status of built `ui.js`/`pkg` (noted in T4 §11).
