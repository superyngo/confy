# Handover — KIND column header, 40% column position, help legend + scrollable help

> **Status:** plan **approved by the user on 2026-06-10** (this session). No implementation
> started. A fresh session may restate the task list for a quick confirm and then implement.

## The three approved changes

User request (verbatim intent) + decisions already made via Q&A:

1. **Rename the `TYPE/FORMAT` column header.** Decision: **`KIND`**.
2. **Move the column left.** Decision: **NAME column takes 40% of the terminal width**, so the
   KIND column starts at the 2/5 mark (today NAME/VALUE split the leftover equally, putting it
   at ~44% on a 100-col terminal). VALUE absorbs the remainder (gets wider).
3. **Explain the tag signs in the `?` help overlay**, and make the help content scrollable so
   the longer text is never cut off on short terminals.

## Where the code stands (2026-06-10)

- Working tree has **uncommitted modifications** (CHANGELOG.md, CLAUDE.md, cst_edit.rs,
  cst_project.rs, node.rs, app.rs, insertion.rs, state.rs, ui.rs) from the just-finished
  "reconstruct increments" work — do not assume a clean tree; build on top of it.
- The KIND-cell content is the fixed-pitch 12-column tag built by `type_tag()` at
  `src/tui/app.rs:2002` — 3-char key sign (`(B)/(Q)/(D)/(-)`) + space + 8-char type slot.
  Full slot inventory (use this for the help legend):
  - Containers: `[G]` root, `[C]` comment, `[A/I]` inline array, `[A/M]` multiline array,
    `[A/T]` array-of-tables, `[T/I]` inline table, `[T/S]` table scope.
  - Scalars `[X:xxxx]`: `[S:str ] [S:mstr] [S:lit ] [S:mlit]`,
    `[I:dec ] [I:hex ] [I:oct ] [I:bin ]`, `[F:flt ] [F:inf ] [F:nan ]`, `[B:bool]`,
    `[D:odt ] [D:ldt ] [D:ldat] [D:ltim]`.

## Task list (approved plan)

### 1. Header rename → `KIND` (`src/tui/ui.rs`)

- `draw_column_header` (`ui.rs:205`): cell text `"TYPE/FORMAT"` → `"KIND"` (`ui.rs:211`).
- Update tests asserting on the header: `ui.rs:784` (`contains("TYPE/FORMAT")`) and the test
  around `ui.rs:864` (`contains("TYPE")`) → assert `KIND`.
- Doc comments that name the *header* ("TYPE/FORMAT column", e.g. `ui.rs:8`, `ui.rs:27`,
  `app.rs:553`) → "KIND column". Don't sweep unrelated comments.

### 2. Column at the 40% mark (`src/tui/ui.rs`, `src/tui/mod.rs`)

- Add `pub(crate) fn name_col_width(total: u16) -> u16 { (total * 2 / 5).max(10) }` (or
  equivalent; keep a sane floor on tiny terminals).
- Both tables — `draw_column_header` (`ui.rs:215-223`) and `draw_tree` (`ui.rs:349-357`) —
  switch constraints from `[Min(10), Length(TYPE_WIDTH), Min(10)]` to
  `[Length(name_col_width(area.width)), Length(TYPE_WIDTH), Min(10)]`, keeping
  `column_spacing(1)`.
- Rework `value_col_width` (`ui.rs:71`) to the exact leftover:
  `total − name_col_width(total) − TYPE_WIDTH − 2` (two 1-col gaps), floor 1 — it feeds the
  inline-editor window, the overflow hint (`ui.rs:415`), and the `/` filter input, so it must
  match what ratatui actually lays out.
- The **Name-field** inline edit currently approximates its width with `value_col_width`
  (`ui.rs:277`); switch it to `name_col_width` (still minus the tree-prefix chars). The
  per-frame scroll clamp in `mod.rs:70-73` also uses `value_col_width` regardless of field;
  make it pick `name_col_width` when `EditState.field == Name` (the prefix subtraction stays
  an approximation there, as today).

### 3. KIND legend in help + scrollable help overlay

- `keys::help_text()` (`src/tui/keys.rs:75`): append a legend section, e.g. a `── KIND ──`
  block: key signs `(B)` bare / `(Q)` quoted / `(D)` dotted / `(-)` keyless, then the container
  and scalar slots from the inventory above with one-line meanings. Keep the existing
  keybinding lines untouched.
- Scrollability: add `help_scroll: u16` to `App` (mirror `detail_scroll`, `app.rs:52`, reset to
  0 on open in `enter_help` at `app.rs:587`); in the Help-mode key handler (`mod.rs:163-172`)
  add `↑/↓ PgUp/PgDn Home/End` adjusting it (clamped like `detail_scroll_by`,
  `app.rs:567-574`); `draw_help_overlay` (`ui.rs:577`) caps the popup at the frame height
  (it already does `.min(f.area().height)`) and renders `Paragraph::…​.scroll((app.help_scroll, 0))`.
  Update the overlay title to advertise scrolling, e.g. `" Help (↑/↓ scroll · ? or Esc) "`.
- Clamp max scroll to `line_count − visible_height` so it can't scroll past the end.

## Verification & wrap-up

- `cargo fmt`, `cargo clippy -- -D warnings`, `cargo test` — all clean.
- User smoke-tests the TUI **manually** — never drive it via pty/background processes.
- Append a CHANGELOG.md `Unreleased Update` entry; update CLAUDE.md where it describes the
  TYPE column (architecture section calls it "the TYPE column renders … fixed-pitch
  12-column tag" — rename to KIND there and note the 40% layout if stating widths).
- Commit only when the user says so; branch is `main`.
