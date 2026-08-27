# Header/Toolbar Chrome — Single Source of Truth

The confy web bundle has one **chrome** (header toolbar + filter row) shared by every
host: plain browser desktop, plain browser/PWA touch, VS Code's webview, and the Tauri
app (desktop + mobile). Each host trims or relocates parts of it differently. This file
is the canonical description of the button inventory, the responsive fold order, and the
per-host trimming/relocation rules — **the fact each of `WEBUI.md`/`VSCODE.md`/`TAURI.md`
otherwise had to restate, and drifted on** (e.g. `TAURI.md` and `VSCODE.md` describing a
filter-row Raw/Tree toggle that no longer defaults there once it moved into the header —
fixed by writing the rule here once instead of three times).

Two markup files implement the chrome and are kept visually/structurally in sync by
convention (not by sharing HTML): `web/index.html` (desktop) and `web/touch/app.ts`'s
`appHTML()` (touch). Desktop identifies controls by element `id`; touch by
`data-act="…"` on the same elements — this file gives both per row.

## Row layout (default: plain browser, no host trimming)

**Row 1 — header (`header.toolbar` / `.toolbar`):** brand, format pill (`fmt-pill`,
`cyclefmt` on touch), dirty-dot, spacer, **Open**, **Save** / **Save As** (desktop:
`.split-btn` with `#btnSave` + `#btnSaveAs`; touch: single Save button that opens a
Save/Save-As action sheet — not part of the fold system below), then `#editGroup` /
`.edit-grp`:

`Theme · Language · Raw/Tree toggle · Help/About` · then `#btnMore` / `[data-act="menu"]`
(the "⋯ More" opener, itself never folds).

**Row 2 — filter row (`.filterbar`):** search box, **Type filter**, then `#navGroup` /
`.nav-grp`, then `#histGroup` / `.hist-grp`:

`search-bar · type-filter · Expand/Collapse · Undo/Redo`

| Control | Desktop id | Touch `data-act` | Row·Group | i18n title key |
|---|---|---|---|---|
| Open | `#btnOpen` | `open` | header (ungrouped) | `web.toolbar.open.title` |
| Save | `#btnSave` | `save` | header (ungrouped) | `web.toolbar.save.title`/`.label` |
| Save As / Convert | `#btnSaveAs` | *(touch: Save sheet's 2nd choice)* | header (ungrouped) | `web.toolbar.saveAs.title` |
| Theme | `#btnTheme` | `theme` | header · `editGroup`/`edit-grp` | `web.toolbar.theme.title` |
| Language | `#btnLang` | `lang` | header · `editGroup`/`edit-grp` | `web.toolbar.lang.title` |
| Raw/Tree toggle | `#btnViewToggle` | `toggleview` | header · `editGroup`/`edit-grp` | `web.toolbar.viewToggle.title` |
| Help / About | `#btnInfo` | `info` | header · `editGroup`/`edit-grp` | `web.toolbar.info.title` |
| More (⋯) | `#btnMore` | `menu` | header (ungrouped, never folds) | `web.toolbar.more.title` |
| Type filter | `#btnTypeFilter` | `filter` | filter row (ungrouped) | `web.toolbar.typefilter.title` |
| Expand all | `#btnExpandAll` | `expandall` | filter row · `navGroup`/`nav-grp` | `web.toolbar.expandAll.title` |
| Collapse all | `#btnCollapseAll` | `collapseall` | filter row · `navGroup`/`nav-grp` | `web.toolbar.collapseAll.title` |
| Undo | `#btnUndo` | `undo` | filter row · `histGroup`/`hist-grp` | `web.toolbar.undo.title` |
| Redo | `#btnRedo` | `redo` | filter row · `histGroup`/`hist-grp` | `web.toolbar.redo.title` |

## Responsive fold order

Both platforms fold the **same buttons in the same relative priority**, right→left, one
button at a time, into the "⋯ More" popup/sheet — just at different numeric thresholds
(desktop: `@media` viewport width; touch: `@container` width of `.app`, a
`container-type:inline-size` container). Widest threshold folds first (least essential);
narrowest folds last (most essential — kept the longest).

| Step | Control | Desktop breakpoint (selector) | Touch breakpoint (selector) |
|---|---|---|---|
| 1 | Raw/Tree toggle | ≤600px (`#btnViewToggle`) | ≤720px (`.edit-grp [data-act="toggleview"]`) |
| 2 | Collapse all | ≤520px (`#btnCollapseAll`) | ≤660px (`.nav-grp [data-act="collapseall"]`) |
| 3 | Expand all | ≤500px (`#btnExpandAll`) | ≤640px (`.nav-grp [data-act="expandall"]`) |
| 4 | Help/About | ≤480px (`#btnInfo`) | ≤620px (`.edit-grp [data-act="info"]`) |
| 5 | Language | ≤460px (`#btnLang`) | ≤600px (`.edit-grp [data-act="lang"]`) |
| 6 | Theme | ≤440px (`#btnTheme`) | ≤580px (`.edit-grp [data-act="theme"]`) |
| 7 | Redo | ≤420px (`#btnRedo`) | ≤560px (`.hist-grp [data-act="redo"]`) |
| 8 | Undo | ≤400px (`#btnUndo`) | ≤540px (`.hist-grp [data-act="undo"]`) |

`#btnMore` (desktop) appears at step 1's threshold; touch's `.more-btn` the same. Each
selector hides **that one button only** (not its whole group) — a button folding never
takes its siblings with it, and the group `<div>`s (`editGroup`/`navGroup`/`histGroup`,
`.edit-grp`/`.nav-grp`/`.hist-grp`) exist purely for flex layout/gap, not as fold units.

**Registry mechanism.** `web/toolbar-fold.ts`'s `foldedEntries(entries, isFolded)` is the
one function both platforms call to build the "⋯ More" popup/sheet contents — it filters
a static `ToolbarEntry[]` list down to whichever entries are currently folded
(`isFolded` = `offsetParent === null`, i.e. the CSS above actually hid it). Desktop's
list is `ui.ts`'s `TOOLBAR_ENTRIES` (keyed by element id); touch's is `touch/app.ts`'s
`MENU_CANDIDATES` (keyed by a `[data-act="…"]` selector). **Every button marked
`data-foldable="true"` in the markup must have a matching entry in the corresponding
list, and vice versa** — enforced by `web/toolbar-fold.spec.mjs`'s structural regression
check (parses both markup and registry, asserts the id/`data-act` sets match).

## Per-host chrome trimming

| Host | Header row 1 | Filter row 2 | Undo/Redo | Raw/Tree toggle | Native replacement |
|---|---|---|---|---|---|
| Web browser **desktop** | visible | visible | visible, `#histGroup` | visible, `#editGroup` | — |
| Web browser **touch**/PWA, **Tauri mobile** | visible | visible | visible, `.hist-grp` | visible, `.edit-grp` | — |
| **VS Code** webview | hidden (`body.host-vscode header.toolbar{display:none}`) | visible | hidden (`body.host-vscode #histGroup{display:none}`) | visible — relocated into the filter row at boot | Undo/Redo: workbench `z`/`y`; Open/Save/Save-As-Convert/Theme/Language/Help-About: editor-title **"…" More Actions** menu (`VSCODE.md` §Chrome trimming) |
| **Tauri desktop** | hidden (`body.host-tauri-desktop header.toolbar{display:none}`) | visible | hidden (`body.host-tauri-desktop #histGroup{display:none}`) | visible — relocated into the filter row at boot | Open/Save/Save-As-Convert/Undo/Redo/Theme/Language/Help-About: native File/Edit/View/Help menu bar (`menu.ts`, `TAURI.md` §Desktop menu); format pill + dirty-dot → native window title |

**Why Undo/Redo and the Raw/Tree toggle need special-casing for VS Code/Tauri desktop:**
both buttons now live in the filter row (`#histGroup`) / header (`#editGroup`)
respectively, and only the *header* is host-hidden — so without an explicit rule each
button would follow whichever row it physically sits in, not whichever row makes sense
for that host:

- **Undo/Redo** sit in the filter row (never hidden), but VS Code/Tauri desktop already
  have a native Undo/Redo entry point and never showed a toolbar copy before either
  button moved rows — so `body.host-vscode #histGroup` / `body.host-tauri-desktop
  #histGroup { display: none }` (`web/style.css`) keeps them hidden in their new location
  too, preventing a new duplicate control neither host asked for.
- **Raw/Tree toggle** sits in the header (now hidden for these two hosts), but neither
  host has a native substitute for it (it toggles the *webview's* tree/raw view, not a
  native editor concept) — so `web/ui.ts`'s `main()` reattaches `#btnViewToggle` to the
  end of `.filterbar` at boot, for `VSHOST || TAURI_DESKTOP`, restoring the same visible
  position it had before this button lived in the header.

Touch is never hosted by VS Code or Tauri desktop (only by a browser, PWA, or Tauri
mobile — none of which trim the chrome), so it carries no relocation/hiding logic at all.

## Adding or moving a toolbar button

Every past change here touched several files at once; missing one is the recurring
doc-drift/behavior-drift bug this file exists to prevent. Checklist:

1. **Markup** — place the button in the right group in both `web/index.html`
   (`id="btnX"`) and `touch/app.ts`'s `appHTML()` (`data-act="x"`), `data-foldable="true"`
   if it should fold.
2. **Fold breakpoint** — add an ID-selector rule to `web/style.css` and a
   `.group [data-act="x"]` rule to `web/touch/style.css`, at the width matching its
   intended essential-ness relative to its neighbors (see the ladder above).
3. **Registry entry** — add a `ToolbarEntry` to `ui.ts`'s `TOOLBAR_ENTRIES` and
   `touch/app.ts`'s `MENU_CANDIDATES` so the "⋯ More" popup/sheet can list it once folded.
4. **Per-host rule, if the button's new row differs from its host-visibility need** — add
   a `body.host-vscode`/`body.host-tauri-desktop` CSS hide, or a `VSHOST`/`TAURI_DESKTOP`
   JS relocation in `ui.ts`'s `main()`, following the pattern above. Skip this only if the
   button's default (hidden-with-its-row or visible-with-its-row) is already what every
   host wants.
5. **Verify** — `node web/toolbar-fold.spec.mjs` (markup/registry parity),
   `node web/run-tests.mjs` (full suite), `node web/build.mjs` (confirms `web/dist` — what
   both the VS Code extension and the Tauri app consume — picked up the change).
6. **Update this file's tables** — row layout, fold ladder, and/or per-host trimming
   table, whichever changed. Leave `WEBUI.md`/`VSCODE.md`/`TAURI.md` pointing here rather
   than re-describing the layout.
