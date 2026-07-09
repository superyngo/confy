# Help/About Panel, Overlay Fix, Info Button, Unified Open Popup — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a tabbed Help/About panel (core-driven, shared by TUI and Web UI), fix the
Web UI's `#overlay` clipping bug, add a header info button, and unify the Web UI's
"Open from URL" modal/sheet with local-file browsing — on both desktop and touch.

**Architecture:** `confy-core`'s `Mode::Help` becomes `Mode::Help(HelpTab)` (`HelpTab::{Help,
About}`), so the TUI and Web UI both render from one authoritative tab value delivered
through the existing `Intent -> dispatch -> SessionSnapshot` channel — no client-only tab
state anywhere. The Web UI's clipping bug and the unified Open popup are pure
HTML/CSS/TypeScript changes with no core involvement.

**Tech Stack:** Rust (confy-core, confy-tui/ratatui), TypeScript (web/, web/touch/, esbuild,
no test framework — `tsc --noEmit` + manual browser verification), wasm-pack.

## Global Constraints

- About text fields (verbatim, from `docs/superpowers/specs/2026-07-09-help-about-open-panel-design.md`):
  version `env!("CARGO_PKG_VERSION")` (currently `0.11.2`), author `wen`, license `MIT`,
  copyright `(c) 2026 wen`, GitHub `https://github.com/superyngo/confy`, tagline
  `A cross-platform TUI/Web UI for editing structured configuration files.`
- Per repo CLAUDE.md: `cargo clippy -- -D warnings` and `cargo fmt --check` must stay clean;
  minimal/surgical changes only, no unrelated refactors.
- Per user's global memory `no-pty-tui-testing`: never drive the TUI via pty or background
  processes in tests — TUI interaction is verified manually by the user, not by an automated
  test in this plan.
- Per user's global memory `esbuild-hangs-on-volume`: the web bundle must be built from a
  scratchpad copy (not directly under `/Volumes/Home`), then copied back.
- Per user's global memory `rebuild-wasm-web-after-core-change`: any confy-core change is not
  "done" until `wasm-pack build --target web` (confy-ffi) and the web bundle are rebuilt and
  `tsc --noEmit` / `functional_smoke.mjs` pass.

---

### Task 1: Core — `HelpTab`, `Mode::Help(HelpTab)`, `ABOUT_TEXT`, `ToggleHelpTab` intent

**Files:**
- Modify: `crates/confy-core/src/session/state.rs`
- Modify: `crates/confy-core/src/session/session.rs:996` (`enter_help`), `:2529` (`escape` match)
- Modify: `crates/confy-core/src/session/dispatch.rs:137-139`, `:356` (`mode_view`)
- Modify: `crates/confy-core/src/session/view.rs` (`ModeView::Help`)
- Modify: `crates/confy-core/src/session/intent.rs`
- Modify: `crates/confy-core/src/session/mod.rs` (re-export `HelpTab`)
- Test: `crates/confy-core/tests/session_headless.rs`

**Interfaces:**
- Produces: `state::HelpTab { Help, About }` (`Clone, Copy, Debug, PartialEq, Eq, Serialize,
  Deserialize`), `state::ABOUT_TEXT: &'static str`, `Mode::Help(HelpTab)`,
  `Session::toggle_help_tab(&mut self)`, `Intent::ToggleHelpTab`,
  `ModeView::Help { tab: HelpTab }`. All re-exported from `confy_core::session`.
- Consumes: nothing new (builds on existing `Mode`, `Session`, `Intent`, `ModeView`, `dispatch`).

- [ ] **Step 1: Add `HelpTab` and `ABOUT_TEXT` to `state.rs`**

  In `crates/confy-core/src/session/state.rs`, change the `Mode` enum's `Help` variant and add
  the new type + const near it:

  ```rust
  /// Which tab of the shared Help/About panel (`?`) is active.
  #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
  pub enum HelpTab {
      Help,
      About,
  }

  /// Static About-tab text: author/version/license/repo, shown alongside Help.
  pub const ABOUT_TEXT: &str = concat!(
      "confy ", env!("CARGO_PKG_VERSION"), "\n",
      "A cross-platform TUI/Web UI for editing structured configuration files.\n",
      "\n",
      "Author:    wen\n",
      "License:   MIT\n",
      "Copyright: (c) 2026 wen\n",
      "GitHub:    https://github.com/superyngo/confy\n",
  );
  ```

  Then change the `Mode` enum's `Help` line from:
  ```rust
      Detail,
      Help,
      Edit(EditState),
  ```
  to:
  ```rust
      Detail,
      Help(HelpTab),
      Edit(EditState),
  ```

- [ ] **Step 2: Update `session.rs`'s `enter_help`, add `toggle_help_tab`, fix `escape` match**

  In `crates/confy-core/src/session/session.rs`, change the import line (currently around
  line 8-11) to include `HelpTab`:
  ```rust
  use crate::session::state::{
      Clipboard, EditField, EditKind, EditState, FilterLayer, History, HelpTab, KindSwitchState,
      Mode, PasteSlot, PendingComment, PendingCommit, PendingExternalEdit, PromptKind,
  };
  ```

  Change `enter_help` (line 996) from `self.mode = Mode::Help;` to:
  ```rust
      pub fn enter_help(&mut self) {
          self.mode = Mode::Help(HelpTab::Help);
      }
  ```

  Immediately after `exit_help` (which stays unchanged), add:
  ```rust
      pub fn toggle_help_tab(&mut self) {
          if let Mode::Help(tab) = &mut self.mode {
              *tab = match tab {
                  HelpTab::Help => HelpTab::About,
                  HelpTab::About => HelpTab::Help,
              };
          }
      }
  ```

  In the `escape` method's match (line 2529), change:
  ```rust
              Mode::Help => self.exit_help(),
  ```
  to:
  ```rust
              Mode::Help(_) => self.exit_help(),
  ```

- [ ] **Step 3: Add `Intent::ToggleHelpTab` and route it in `dispatch.rs`**

  In `crates/confy-core/src/session/intent.rs`, add a variant next to `HelpScrollBy`/
  `HelpSetScroll`:
  ```rust
      HelpScrollBy(i32, u16),
      HelpSetScroll(u16),
      /// Flip the shared Help/About panel between its two tabs (TUI `Tab` key /
      /// Web UI tab-button click), while `Mode::Help(_)` is active.
      ToggleHelpTab,
  ```

  In `crates/confy-core/src/session/dispatch.rs`, change lines 137-139 from:
  ```rust
              Intent::EnterHelp => self.enter_help(),
              Intent::ExitHelp => self.exit_help(),

              Intent::HelpScrollBy(..) | Intent::HelpSetScroll(..) => {}
  ```
  to:
  ```rust
              Intent::EnterHelp => self.enter_help(),
              Intent::ExitHelp => self.exit_help(),
              Intent::ToggleHelpTab => self.toggle_help_tab(),

              Intent::HelpScrollBy(..) | Intent::HelpSetScroll(..) => {}
  ```

  In the same file's `mode_view` match (line 356), change:
  ```rust
              Mode::Detail => ModeView::Detail,
              Mode::Help => ModeView::Help,
  ```
  to:
  ```rust
              Mode::Detail => ModeView::Detail,
              Mode::Help(tab) => ModeView::Help { tab: *tab },
  ```

- [ ] **Step 4: Update `ModeView::Help` in `view.rs`**

  In `crates/confy-core/src/session/view.rs`, add `HelpTab` to the `use` line (currently
  `use crate::session::state::{ConvertStep, EditField};`):
  ```rust
  use crate::session::state::{ConvertStep, EditField, HelpTab};
  ```

  Change the `ModeView` enum's Help variant from:
  ```rust
      /// The `?` help overlay is open.
      Help,
  ```
  to:
  ```rust
      /// The `?` help overlay is open, on tab `tab`.
      Help { tab: HelpTab },
  ```

- [ ] **Step 5: Re-export `HelpTab` from `session/mod.rs`**

  In `crates/confy-core/src/session/mod.rs`, add `HelpTab` to the `pub use state::{...}` list
  (alphabetically, between `History` and `KindSwitchState`):
  ```rust
  pub use state::{
      Clipboard, ConvertState, ConvertStep, EditField, EditKind, EditState, FilterLayer,
      HelpTab, History, KindSwitchState, Mode, PasteSlot, PendingComment, PendingCommit,
      PendingExternalEdit, PromptKind,
  };
  ```

- [ ] **Step 6: Write the failing tests**

  Append to `crates/confy-core/tests/session_headless.rs` (it already imports `Mode`,
  `ModeView`, `Intent` from `confy_core::session` at the top — add `HelpTab` there too):
  ```rust
  use confy_core::session::{EditKind, EditTextOutcome, Host, HelpTab, Intent, Mode, ModeView, Session};
  ```

  Then add at the end of the file:
  ```rust
  #[test]
  fn enter_help_defaults_to_help_tab_and_toggle_flips_to_about() {
      let mut s = toml_session("a = 1\n");
      s.dispatch(Intent::EnterHelp);
      assert!(matches!(s.mode, Mode::Help(HelpTab::Help)));
      s.dispatch(Intent::ToggleHelpTab);
      assert!(matches!(s.mode, Mode::Help(HelpTab::About)));
      s.dispatch(Intent::ToggleHelpTab);
      assert!(matches!(s.mode, Mode::Help(HelpTab::Help)));
  }

  #[test]
  fn dispatch_snapshot_carries_help_tab() {
      let mut s = toml_session("a = 1\n");
      let snap = s.dispatch(Intent::EnterHelp);
      assert!(matches!(snap.mode, ModeView::Help { tab: HelpTab::Help }));
      let snap = s.dispatch(Intent::ToggleHelpTab);
      assert!(matches!(snap.mode, ModeView::Help { tab: HelpTab::About }));
  }

  #[test]
  fn toggle_help_tab_is_noop_outside_help_mode() {
      let mut s = toml_session("a = 1\n");
      s.dispatch(Intent::ToggleHelpTab);
      assert!(matches!(s.mode, Mode::Normal));
  }

  #[test]
  fn escape_exits_help_from_either_tab() {
      let mut s = toml_session("a = 1\n");
      s.dispatch(Intent::EnterHelp);
      s.dispatch(Intent::ToggleHelpTab);
      s.dispatch(Intent::Escape);
      assert!(matches!(s.mode, Mode::Normal));
  }
  ```

- [ ] **Step 7: Run the tests to verify they fail to compile first, then pass**

  Run: `cargo test -p confy-core --test session_headless help`
  Expected (before Steps 1-5, if run first): compile error `Mode::Help` used as unit variant / no
  `HelpTab` in scope. After Steps 1-5 are in place: `4 passed`.

- [ ] **Step 8: Full workspace check**

  Run: `cargo build && cargo test -p confy-core && cargo clippy -- -D warnings && cargo fmt --check`
  Expected: all green. Fix any clippy/fmt issues introduced by the new code (e.g. import
  ordering) before proceeding.

- [ ] **Step 9: Commit**

  ```bash
  git add crates/confy-core/src/session/state.rs crates/confy-core/src/session/session.rs \
          crates/confy-core/src/session/dispatch.rs crates/confy-core/src/session/view.rs \
          crates/confy-core/src/session/intent.rs crates/confy-core/src/session/mod.rs \
          crates/confy-core/tests/session_headless.rs
  git commit -m "feat(core): add HelpTab and shared Help/About Mode"
  ```

---

### Task 2: Web UI — fix `#overlay` clipping/z-index bug

**Files:**
- Modify: `web/index.html`
- Modify: `web/style.css`

**Interfaces:**
- Consumes: nothing (pure markup/CSS move, independent of Task 1).
- Produces: `#overlay` is a direct child of `<body>` (sibling of `.main`), `position: fixed`.

- [ ] **Step 1: Move `<aside id="overlay">` out of `.main` in `web/index.html`**

  Current (around line 91-110):
  ```html
      <!-- ===== main ===== -->
      <div class="main">
        <div class="tree-wrap" id="treeWrap">
          <div id="tree" tabindex="0"></div>
          <pre id="raw" class="raw-view mono hidden" tabindex="0" aria-readonly="true"></pre>
          <div class="marquee" id="marquee"></div>
          <div class="drop-line" id="dropLine"></div>
        </div>
        <aside class="detail" id="detail">
          <div class="detail-head">
            <h3 id="detailTitle">Node detail</h3>
            <button class="icon-btn" id="detailClose" title="close">
              <svg class="ic" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M6 6l12 12M18 6 6 18"/></svg>
            </button>
          </div>
          <div class="detail-body" id="detailBody"></div>
        </aside>
        <!-- Fallback overlay for modes not yet redesigned (keyboard-driven). -->
        <aside id="overlay" class="hidden"></aside>
      </div>
  ```

  Change to (overlay removed from inside `.main`, re-added right after it):
  ```html
      <!-- ===== main ===== -->
      <div class="main">
        <div class="tree-wrap" id="treeWrap">
          <div id="tree" tabindex="0"></div>
          <pre id="raw" class="raw-view mono hidden" tabindex="0" aria-readonly="true"></pre>
          <div class="marquee" id="marquee"></div>
          <div class="drop-line" id="dropLine"></div>
        </div>
        <aside class="detail" id="detail">
          <div class="detail-head">
            <h3 id="detailTitle">Node detail</h3>
            <button class="icon-btn" id="detailClose" title="close">
              <svg class="ic" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M6 6l12 12M18 6 6 18"/></svg>
            </button>
          </div>
          <div class="detail-body" id="detailBody"></div>
        </aside>
      </div>
      <!-- Fallback overlay for modes not yet redesigned (keyboard-driven). Lives
           outside `.main` (which is `overflow:hidden`) so a tall popup is never
           clipped by it; `position:fixed` centers it on the viewport instead. -->
      <aside id="overlay" class="hidden"></aside>
  ```

- [ ] **Step 2: Change `#overlay` CSS to `position: fixed`, viewport-centered**

  In `web/style.css`, change (around line 392-397):
  ```css
  #overlay {
    position: absolute; left: 50%; top: 40%; transform: translate(-50%, -50%);
    background: var(--surface); border: 1px solid var(--border-strong);
    border-radius: 12px; padding: 16px 20px; max-width: 70vw; max-height: 70vh;
    overflow: auto; box-shadow: var(--shadow); z-index: 40;
  }
  ```
  to:
  ```css
  #overlay {
    position: fixed; left: 50%; top: 50%; transform: translate(-50%, -50%);
    background: var(--surface); border: 1px solid var(--border-strong);
    border-radius: 12px; padding: 16px 20px; max-width: 70vw; max-height: 70vh;
    overflow: auto; box-shadow: var(--shadow); z-index: 40;
  }
  ```

- [ ] **Step 3: Rebuild and manually verify**

  Per the `esbuild-hangs-on-volume` constraint, build from a scratchpad copy:
  ```bash
  rm -rf /tmp/confy-web-build && cp -R /Volumes/Home/Users/wen/repos/confy/web /tmp/confy-web-build
  cd /tmp/confy-web-build && npm install && npm run build
  cp ui.js ui.js.map touch/app.js touch/app.js.map /Volumes/Home/Users/wen/repos/confy/web/ 2>/dev/null || true
  cp -R /tmp/confy-web-build/dist/. /Volumes/Home/Users/wen/repos/confy/web/dist/ 2>/dev/null || true
  ```
  Then: `cd /Volumes/Home/Users/wen/repos/confy/web && npx serve.mjs` (or whatever the existing
  `serve` script is) and open the page; press `?` on a short browser window to confirm the Help
  overlay's top edge is no longer clipped/covered by the header. Report this verification to
  the user — do not claim it fixed without having actually looked at it in a browser, per the
  project's UI-testing convention.

- [ ] **Step 4: Commit**

  ```bash
  git add web/index.html web/style.css
  git commit -m "fix(web): stop #overlay clipping under the header"
  ```

---

### Task 3: TUI — `Tab` switches Help/About, dual-tab rendering

**Depends on:** Task 1 (`HelpTab`, `Mode::Help(HelpTab)`, `ABOUT_TEXT`).

**Files:**
- Modify: `crates/confy-tui/src/tui/state.rs` (re-export list)
- Modify: `crates/confy-tui/src/tui/mod.rs:170-186` (Help key-handling branch)
- Modify: `crates/confy-tui/src/tui/ui.rs:637-654` (`draw_help_overlay`)
- Modify: `crates/confy-tui/src/tui/app.rs` (no signature change needed; `toggle_help_tab`
  is called straight on `app.session`)

**Interfaces:**
- Consumes: `confy_core::session::state::HelpTab`, `Session::toggle_help_tab()`,
  `Mode::Help(HelpTab)` (via `app.session.mode`), `state::ABOUT_TEXT`.
- Produces: nothing new consumed elsewhere (leaf UI change).

- [ ] **Step 1: Add `HelpTab`/`ABOUT_TEXT` to `crates/confy-tui/src/tui/state.rs`'s re-export list**

  This file is a named re-export list (not a glob), currently:
  ```rust
  // Re-exported from confy-core — the pure state types live there.
  pub use confy_core::session::state::{
      Clipboard, ConvertState, ConvertStep, EditField, EditState, FilterLayer, History,
      KindSwitchState, Mode, PasteSlot, PendingComment, PromptKind,
  };
  ```
  Change to:
  ```rust
  // Re-exported from confy-core — the pure state types live there.
  pub use confy_core::session::state::{
      Clipboard, ConvertState, ConvertStep, EditField, EditState, FilterLayer, HelpTab, History,
      KindSwitchState, Mode, PasteSlot, PendingComment, PromptKind, ABOUT_TEXT,
  };
  ```

- [ ] **Step 2: Update the `Mode::Help` match guards to the tuple variant**

  In `crates/confy-tui/src/tui/mod.rs`, line 170, change:
  ```rust
              if matches!(app.session.mode, crate::tui::state::Mode::Help) {
  ```
  to:
  ```rust
              if matches!(app.session.mode, crate::tui::state::Mode::Help(_)) {
  ```

  In `crates/confy-tui/src/tui/ui.rs`, line 637, change:
  ```rust
      if !matches!(app.session.mode, Mode::Help) {
  ```
  to:
  ```rust
      if !matches!(app.session.mode, Mode::Help(_)) {
  ```

- [ ] **Step 3: Add `Tab`/`BackTab` handling in the Help key branch**

  In `crates/confy-tui/src/tui/mod.rs`, the Help branch (lines 170-186) computes `help_lines`
  from `keys::help_text(app.doc_format())` unconditionally — this must now depend on which tab
  is active, since `ABOUT_TEXT` is a different length. Replace the whole branch body with:
  ```rust
              if matches!(app.session.mode, crate::tui::state::Mode::Help(_)) {
                  use crossterm::event::KeyCode;
                  let active_tab = match app.session.mode {
                      crate::tui::state::Mode::Help(t) => t,
                      _ => unreachable!(),
                  };
                  let text = match active_tab {
                      crate::tui::state::HelpTab::Help => keys::help_text(app.doc_format()),
                      crate::tui::state::HelpTab::About => crate::tui::state::ABOUT_TEXT,
                  };
                  let help_lines = text.lines().count() as u16;
                  // Approximate visible height: terminal height minus 2 borders.
                  let inner_h = terminal.size()?.height.saturating_sub(2);
                  let max_scroll = help_lines.saturating_sub(inner_h);
                  let page = inner_h.max(1) as i32;
                  match key.code {
                      KeyCode::Down | KeyCode::Char('j') => app.help_scroll_by(1, max_scroll),
                      KeyCode::Up | KeyCode::Char('k') => app.help_scroll_by(-1, max_scroll),
                      KeyCode::PageDown => app.help_scroll_by(page, max_scroll),
                      KeyCode::PageUp => app.help_scroll_by(-page, max_scroll),
                      KeyCode::Home => app.help_set_scroll(0),
                      KeyCode::End => app.help_set_scroll(max_scroll),
                      KeyCode::Tab | KeyCode::BackTab => {
                          app.session.toggle_help_tab();
                          app.help_set_scroll(0);
                      }
                      KeyCode::Esc | KeyCode::Char('?') => app.escape(),
                      _ => {}
                  }
                  continue;
              }
  ```

- [ ] **Step 4: Render the active tab in `draw_help_overlay`**

  In `crates/confy-tui/src/tui/ui.rs`, replace the function (lines ~636-654):
  ```rust
  fn draw_help_overlay(f: &mut Frame, app: &App) {
      if !matches!(app.session.mode, Mode::Help(_)) {
          return;
      }
      let tab = match app.session.mode {
          Mode::Help(t) => t,
          _ => unreachable!(),
      };
      use crate::tui::state::HelpTab;
      let (title, text) = match tab {
          HelpTab::Help => (
              " Help | About (Tab to switch · ↑/↓ scroll · ? or Esc) ",
              keys::help_text(app.doc_format()),
          ),
          HelpTab::About => (
              " About | Help (Tab to switch · ↑/↓ scroll · ? or Esc) ",
              crate::tui::state::ABOUT_TEXT,
          ),
      };
      let line_count = text.lines().count() as u16;
      let height = (line_count + 2).min(f.area().height);
      let area = centered_rect(65, height, f.area());
      f.render_widget(Clear, area);
      let block = Block::default()
          .title(title)
          .borders(Borders::ALL)
          .style(Style::default().bg(Color::Black).fg(Color::White));
      let paragraph = Paragraph::new(text)
          .block(block)
          .scroll((app.help_scroll, 0));
      f.render_widget(paragraph, area);
  }
  ```

- [ ] **Step 5: Build and run existing tests**

  Run: `cargo build -p confy-tui && cargo test -p confy-tui && cargo clippy -p confy-tui -- -D warnings && cargo fmt --check`
  Expected: all green (no TUI interaction tests exist or are added, per the
  `no-pty-tui-testing` constraint above).

- [ ] **Step 6: Ask the user to manually verify**

  Tell the user to run `cargo run -- <some-file.toml>`, press `?` (see Help), press `Tab` (see
  About with author/version/license/GitHub), press `Tab` again (back to Help), press `Esc` to
  close. Do not mark this task's manual-verification checkbox done until the user confirms.

- [ ] **Step 7: Commit**

  ```bash
  git add crates/confy-tui/src/tui/mod.rs crates/confy-tui/src/tui/ui.rs
  git commit -m "feat(tui): Tab switches the ? overlay between Help and About"
  ```

---

### Task 4: Web UI — Help/About tabs in `#overlay` + header info button

**Depends on:** Task 1 (`ModeView::Help { tab }`), Task 2 (fixed `#overlay` positioning).

**Files:**
- Create: `web/help-content.ts` (new shared module: `HELP_TEXT`, `ABOUT_TEXT`, `KIND_LEGEND`)
- Modify: `web/types.ts` (mirror `ModeView::Help`)
- Modify: `web/ui.ts` (`renderOverlay`, wiring; `HELP_TEXT`/`KIND_LEGEND` moved out)
- Modify: `web/index.html` (header info button)
- Modify: `web/style.css` (tab button styles)

**Interfaces:**
- Consumes: `snap.mode` as `{ Help: { tab: "Help" | "About" } }` (new shape), `send(intent)`
  helper already in `ui.ts`.
- Produces: `web/help-content.ts` exports `HELP_TEXT: string`, `ABOUT_TEXT: string`,
  `KIND_LEGEND: Record<string, string>` — consumed here and by Task 7 (touch).

- [ ] **Step 1: Update the `ModeView` mirror in `web/types.ts`**

  Change (line ~121):
  ```ts
    | "Help"
  ```
  to:
  ```ts
    | { Help: { tab: "Help" | "About" } }
  ```

- [ ] **Step 2: Extract `web/help-content.ts`, adding `ABOUT_TEXT`**

  Create `web/help-content.ts` by moving the existing `HELP_TEXT` and `KIND_LEGEND` consts out
  of `web/ui.ts` (currently at lines 1378-1400 and 1424-1454) verbatim, exporting all three:
  ```ts
  export const HELP_TEXT = `confy web — keys
  j/k or ↑/↓     move cursor
  Enter/Space    toggle branch / edit leaf / activate
  e              edit (inline or multiline modal)
  a              add node · d delete · c copy · x cut · v paste
  r              remark (toggle node ↔ comment)
  +/- or ←/→     nudge numeric value
  z / y          undo / redo
  s              toggle select · 0 collapse-all · 9 expand-all
  1 / 2          expand / collapse one level
  /              filter · f type-filter · K kind-switch · C convert
  i              detail popup · ? this help · Ctrl-s save · Ctrl-o open
  q              quit (prompts if dirty)

  ── pointer ──────────────────────────────────────
  click          select          ⇧click   range-select
  ⌘click         multi-select    drag     marquee / move
  right-click    context menu

  Open (Ctrl-o) and in-place Save need the File System Access API
  (Chrome/Edge). Other browsers fall back to the paste-load / download path.`;

  // Static About-tab text — keep in sync with
  // crates/confy-core/src/session/state.rs::ABOUT_TEXT. The Web UI has no Cargo
  // build step to inject env!("CARGO_PKG_VERSION") automatically, so the version
  // string here is updated by hand on release; a brief drift is accepted, not a bug.
  export const ABOUT_TEXT = `confy 0.11.2
  A cross-platform TUI/Web UI for editing structured configuration files.

  Author:    wen
  License:   MIT
  Copyright: (c) 2026 wen
  GitHub:    https://github.com/superyngo/confy`;

  // Per-format KIND legend appended to the Help overlay, keyed by `doc_format`
  // (ported from the TUI's TOML_HELP/JSON_HELP/YAML_HELP KIND column). The kind
  // badge shows the friendly label + notation suffix; this explains what each
  // notation means for the open file's backend.
  export const KIND_LEGEND: Record<string, string> = {
    Toml: `── KIND badge (TOML) ──────────────────────────────
  Containers (label·notation):
    table·scope    standard [header] table
    table·dotted   dotted-key table (a.b.c = …)
    inline         inline table { … }
    array·inline   inline array        array·multi  multiline array
    AoT            array-of-tables  [[…]]

  Scalars (label·notation):
    str            basic string        str·"…"  (quoted)
    str·'…'        literal string
    str·"""        multiline basic     str·'''  multiline literal
    int            decimal integer
    int·0x int·0o int·0b   hex / octal / binary
    float / float·dec      float        float·1e  exponent
    float·inf float·nan    infinity / NaN
    bool · date · time · null`,
    Json: `── KIND badge (JSON / JSONC) ──────────────────────
  Containers (label·notation):
    table          object { … }        table·multi  multiline object
    inline         inline object
    array·inline   inline array        array·multi  multiline array

  Scalars (label·notation):
    str            string              null
    int            integer
    float          float               float·1e  exponent
    bool`,
    Yaml: `── KIND badge (YAML) ──────────────────────────────
  Containers (label·notation):
    table·block    block mapping       table·flow  flow mapping { … }
    array·block    block sequence      array·flow  flow sequence [ … ]
    (opaque nodes — anchors/aliases/merge/tags — are read-only)

  Scalars (label·notation):
    str            plain string        str·'…'  single-quoted`,
  };
  ```
  Copy the exact remaining lines of the existing `KIND_LEGEND` values from `web/ui.ts:1424-1454`
  (the `str·"""`/`float·inf`/etc. rows shown above are the head of each block — copy every line
  through the const's closing `};`, do not truncate any format's legend).

  Delete the old `const HELP_TEXT = ...` and `const KIND_LEGEND = ...` from `web/ui.ts`, and add
  an import near the top of `web/ui.ts` (with the other local imports):
  ```ts
  import { HELP_TEXT, ABOUT_TEXT, KIND_LEGEND } from "./help-content.js";
  ```

- [ ] **Step 3: Rewrite the `"Help"` branch of `renderOverlay` in `web/ui.ts`**

  Replace the `renderOverlay` function (lines 283-315) — the whole function — with:
  ```ts
  function renderOverlay() {
    const m = snap!.mode;
    const tag = modeTag(m);
    if (tag === "Help" || tag === "Prompt" || tag === "KindSwitch") {
      overlay.classList.remove("hidden");
    } else {
      overlay.classList.add("hidden");
      return;
    }
    if (tag === "Help") {
      const activeTab = (m as { Help: { tab: "Help" | "About" } }).Help.tab;
      const legend = KIND_LEGEND[snap!.doc_format] ?? "";
      const body =
        activeTab === "Help" ? HELP_TEXT + "\n" + legend : ABOUT_TEXT;
      overlay.innerHTML =
        `<div class="help-tabs">` +
        `<button class="opt tab-btn${activeTab === "Help" ? " sel" : ""}" data-tab="Help">Help</button>` +
        `<button class="opt tab-btn${activeTab === "About" ? " sel" : ""}" data-tab="About">About</button>` +
        `</div><pre>${escapeHtml(body)}</pre>`;
      overlay.querySelectorAll<HTMLElement>("[data-tab]").forEach((btn) => {
        btn.addEventListener("click", () => send("ToggleHelpTab"));
      });
    } else if (tag === "Prompt") {
      const kind = (m as { Prompt: { kind: PromptView } }).Prompt.kind;
      overlay.innerHTML =
        `<h3>${escapeHtml(promptQuestion(kind, snap!.status ?? snap!.error ?? undefined))}</h3>` +
        promptButtonsHTML(kind);
    } else if (tag === "KindSwitch") {
      const ks = (m as { KindSwitch: { cursor: number; options: { label: string }[] } })
        .KindSwitch;
      overlay.innerHTML =
        `<h3>Kind</h3>` +
        ks.options
          .map(
            (o, i) =>
              `<div class="opt${i === ks.cursor ? " sel" : ""}">${escapeHtml(o.label)}</div>`,
          )
          .join("");
    }
  }
  ```
  (`send` must accept a bare string intent tag the way other zero-payload intents already do
  elsewhere in this file — check an existing call like `send("EnterHelp")` at the `?` binding,
  used the same way here, so no new helper is needed.)

- [ ] **Step 4: Add the header info button in `web/index.html`**

  In the `.toolbar` (around line 44-46, in the `tgroup` with undo/redo/theme), add a fourth
  button:
  ```html
      <div class="tgroup" id="editGroup">
        <button class="icon-btn" id="btnUndo" title="Undo (z)">
          <svg class="ic" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M9 7L4 12l5 5"/><path d="M4 12h11a5 5 0 0 1 0 10h-3"/></svg>
        </button>
        <button class="icon-btn" id="btnRedo" title="Redo (y)">
          <svg class="ic" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M15 7l5 5-5 5"/><path d="M20 12H9a5 5 0 0 0 0 10h3"/></svg>
        </button>
        <button class="icon-btn" id="btnTheme" title="Toggle theme">
          <svg class="ic" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 12.8A9 9 0 1 1 11.2 3 7 7 0 0 0 21 12.8z"/></svg>
        </button>
        <button class="icon-btn" id="btnInfo" title="Help / About (?)">
          <svg class="ic" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="9"/><path d="M12 11v6"/><path d="M12 7.5h.01"/></svg>
        </button>
      </div>
  ```

- [ ] **Step 5: Wire the button and add tab-button CSS**

  In `web/ui.ts`, near the other button wiring (around line 1240, next to `themeBtn.addEventListener(...)`):
  ```ts
    $("btnInfo").addEventListener("click", () => send("EnterHelp"));
  ```

  In `web/style.css`, add near the `#overlay` rules (after the existing `#overlay .opt.sel`
  rule at line 401):
  ```css
  #overlay .help-tabs { display: flex; gap: 6px; margin-bottom: 10px; }
  #overlay .tab-btn { border: 1px solid var(--border-strong); background: var(--panel); }
  #overlay .tab-btn.sel { background: var(--accent); color: var(--bg); border-color: transparent; }
  ```

- [ ] **Step 6: `tsc --noEmit`, rebuild, manual verify**

  Run: `cd web && npx tsc --noEmit` — expected: no errors.
  Rebuild per the scratchpad procedure in Task 2 Step 3, then in a browser: click the new info
  button (Help tab shows keybindings), click "About" (shows version/author/license/GitHub),
  click "Help" again, press Esc to close. Report actual browser verification to the user.

- [ ] **Step 7: Commit**

  ```bash
  git add web/help-content.ts web/types.ts web/ui.ts web/index.html web/style.css
  git commit -m "feat(web): Help/About tabs + header info button"
  ```

---

### Task 5: Web UI (desktop) — unified Open popup (URL + local file)

**Files:**
- Modify: `web/index.html` (`#url-modal` -> combined Open modal)
- Modify: `web/ui.ts` (`openUrlModal` -> `openOpenModal`, wiring, More-menu entry removed)
- Modify: `web/style.css` (small divider style)

**Interfaces:**
- Consumes: existing `doOpen()` (local file), `openFromUrl(io, openText, url)` (URL fetch) —
  unchanged signatures.
- Produces: nothing new consumed elsewhere (leaf UI change; independent of Tasks 1-4).

- [ ] **Step 1: Restructure `#url-modal` markup in `web/index.html`**

  Change (lines 171-180):
  ```html
      <!-- ===== URL open modal: fetch a remote config (More ▸ Open from URL) ===== -->
      <div id="url-modal" class="modal hidden">
        <div class="modal-box">
          <h3>Open from URL</h3>
          <input id="url-input" type="url" placeholder="https://example.com/config.toml" />
          <div class="modal-actions">
            <button id="url-confirm">Open</button>
            <button id="url-cancel">Cancel</button>
          </div>
        </div>
      </div>
  ```
  to:
  ```html
      <!-- ===== Open modal: local file browse or fetch a remote config ===== -->
      <div id="url-modal" class="modal hidden">
        <div class="modal-box">
          <h3>Open</h3>
          <button id="url-browse" class="tbtn">Browse local file…</button>
          <div class="modal-divider">or open from URL</div>
          <input id="url-input" type="url" placeholder="https://example.com/config.toml" />
          <div class="modal-actions">
            <button id="url-confirm">Open</button>
            <button id="url-cancel">Cancel</button>
          </div>
        </div>
      </div>
  ```

- [ ] **Step 2: Add the divider CSS**

  In `web/style.css`, after the `.modal-box h3` rule (line 447):
  ```css
  .modal-divider {
    font-size: 11px; color: var(--muted); text-align: center;
    display: flex; align-items: center; gap: 8px;
  }
  .modal-divider::before, .modal-divider::after {
    content: ""; flex: 1; height: 1px; background: var(--border);
  }
  ```

- [ ] **Step 3: Rename and rewire `openUrlModal` in `web/ui.ts`, add browse handler**

  Change the function (lines 705-711) from:
  ```ts
  function openUrlModal() {
    const input = $<HTMLInputElement>("url-input");
    input.value = "";
    $("url-modal").classList.remove("hidden");
    input.focus();
  }
  ```
  to:
  ```ts
  function openOpenModal() {
    const input = $<HTMLInputElement>("url-input");
    input.value = "";
    $("url-modal").classList.remove("hidden");
    input.focus();
  }
  ```
  (keep every existing call site working by search-and-replacing `openUrlModal` ->
  `openOpenModal` throughout `web/ui.ts` — there is exactly one other reference, the More-menu
  item removed in Step 5 below, so after that removal this rename has no remaining call sites
  except the header button wired in Step 4.)

  Repoint the header Open button (line ~1231):
  ```ts
    openBtn.addEventListener("click", () => void doOpen());
  ```
  to:
  ```ts
    openBtn.addEventListener("click", openOpenModal);
  ```

  Add the browse-button handler next to the existing `url-confirm`/`url-cancel` wiring
  (around line 1252-1260):
  ```ts
    $("url-browse").addEventListener("click", () => {
      $("url-modal").classList.add("hidden");
      void doOpen();
    });
  ```

- [ ] **Step 4: Remove the now-redundant More-menu entry**

  In `web/ui.ts`'s `buildMoreMenu` (around line 1112-1120), remove the line:
  ```ts
      ["Open from URL…", openUrlModal],
  ```
  (Update the reference to use the renamed function if any remains; after this removal there
  should be no other call sites of the old name, so `openOpenModal` is only invoked from the
  header `btnOpen` handler above.)

- [ ] **Step 5: `tsc --noEmit`, rebuild, manual verify**

  Run: `cd web && npx tsc --noEmit` — expected: no errors (no leftover reference to
  `openUrlModal`).
  Rebuild per Task 2 Step 3's scratchpad procedure, then in a browser: click the header "Open"
  button — the combined popup appears with "Browse local file…" and the URL field; click
  Browse (native file picker opens, popup closes); reopen, type a URL, click Open (fetches);
  confirm the More (⋯) menu no longer has "Open from URL…". Report actual verification to the
  user.

- [ ] **Step 6: Commit**

  ```bash
  git add web/index.html web/ui.ts web/style.css
  git commit -m "feat(web): unify Open-from-URL modal with local-file browse"
  ```

---

### Task 6: Touch — unified Open sheet (URL + local file)

**Files:**
- Modify: `web/touch/app.ts`

**Interfaces:**
- Consumes: existing `doOpen()`, `openFromUrl(url)` — unchanged.
- Produces: nothing new consumed elsewhere; independent of Task 5 (separate touch codebase per
  the module map's design).

- [ ] **Step 1: Rename `openUrlSheet` to `openOpenSheet`, add the Browse button**

  Change the function (lines 665-692) from:
  ```ts
  function openUrlSheet() {
    if (sheets.url.classList.contains("open")) return;
    sheets.url.innerHTML =
      '<div class="grab"></div>' +
      `<div class="sheet-head"><h3>Open from URL</h3><button class="close" data-act="closesheet">${IC.close}</button></div>` +
      '<div class="sheet-body">' +
      '<input class="url-input" type="url" inputmode="url" spellcheck="false" autocomplete="off" autocapitalize="off" placeholder="https://example.com/config.toml" />' +
      '<div class="row-btns"><button class="btn" data-act="closesheet">Cancel</button>' +
      '<button class="btn primary url-open">Open</button></div>' +
      "</div>";
    const inp = sheets.url.querySelector<HTMLInputElement>(".url-input")!;
    const go = () => {
      const url = inp.value.trim();
      closeSheets();
      if (url) void openFromUrl(url);
    };
    // Open is wired directly (no data-act) so shell delegation never double-fires.
    sheets.url.querySelector<HTMLElement>(".url-open")!.onclick = go;
    inp.addEventListener("keydown", (e) => {
      if (e.key === "Enter") {
        e.preventDefault();
        go();
      }
    });
    openSheet("url");
    inp.focus();
  }
  ```
  to:
  ```ts
  function openOpenSheet() {
    if (sheets.url.classList.contains("open")) return;
    sheets.url.innerHTML =
      '<div class="grab"></div>' +
      `<div class="sheet-head"><h3>Open</h3><button class="close" data-act="closesheet">${IC.close}</button></div>` +
      '<div class="sheet-body">' +
      '<button class="btn browse-local">Browse local file…</button>' +
      '<div class="sheet-divider">or open from URL</div>' +
      '<input class="url-input" type="url" inputmode="url" spellcheck="false" autocomplete="off" autocapitalize="off" placeholder="https://example.com/config.toml" />' +
      '<div class="row-btns"><button class="btn" data-act="closesheet">Cancel</button>' +
      '<button class="btn primary url-open">Open</button></div>' +
      "</div>";
    const inp = sheets.url.querySelector<HTMLInputElement>(".url-input")!;
    const go = () => {
      const url = inp.value.trim();
      closeSheets();
      if (url) void openFromUrl(url);
    };
    sheets.url.querySelector<HTMLElement>(".browse-local")!.onclick = () => {
      closeSheets();
      void doOpen();
    };
    // Open is wired directly (no data-act) so shell delegation never double-fires.
    sheets.url.querySelector<HTMLElement>(".url-open")!.onclick = go;
    inp.addEventListener("keydown", (e) => {
      if (e.key === "Enter") {
        e.preventDefault();
        go();
      }
    });
    openSheet("url");
    inp.focus();
  }
  ```

- [ ] **Step 2: Repoint the header Open button and remove the More-menu duplicate**

  In `installShellHandlers`'s switch (around line 1174-1179), change:
  ```ts
        case "open":
          void doOpen();
          break;
  ```
  to:
  ```ts
        case "open":
          openOpenSheet();
          break;
  ```

  In `openMenuSheet` (around line 560-586), remove the folded "Open from URL" item — delete
  this line:
  ```ts
      mi(linkIc, "Open from URL", "", "url") +
  ```
  and this branch inside the click handler right below it:
  ```ts
        if (id === "url") {
          closeSheets();
          openUrlSheet();
          return;
        }
  ```
  `linkIc` (the `const linkIc = '<svg ...>...</svg>';` declared just above the `sheets.menu.innerHTML =`
  assignment) has no other reference in the file once this is deleted — delete its declaration
  too, otherwise `tsc`/lint will flag it as unused.

- [ ] **Step 3: Add the sheet-divider CSS**

  In `web/touch/style.css`, add (matching the desktop `.modal-divider` from Task 5 Step 2):
  ```css
  .sheet-divider {
    font-size: 11px; color: var(--muted); text-align: center;
    display: flex; align-items: center; gap: 8px; margin: 4px 0;
  }
  .sheet-divider::before, .sheet-divider::after {
    content: ""; flex: 1; height: 1px; background: var(--border);
  }
  ```
  (`--muted` and `--border` are the same custom-property names `web/touch/style.css` already
  defines at lines 16-17/27-28, matching desktop `style.css`'s convention.)

- [ ] **Step 4: `tsc --noEmit`, rebuild, manual verify on a touch/coarse-pointer device or emulation**

  Run: `cd web && npx tsc --noEmit` — expected: no errors.
  Rebuild per Task 2 Step 3's scratchpad procedure (this also rebuilds `touch/app.js`). In a
  browser with device emulation (coarse pointer), tap "Open" in the header — the combined
  sheet opens with "Browse local file…" and the URL field; tapping Browse opens the native
  file picker; the "More actions" sheet no longer lists "Open from URL". Report actual
  verification to the user.

- [ ] **Step 5: Commit**

  ```bash
  git add web/touch/app.ts web/touch/style.css
  git commit -m "feat(touch): unify Open-from-URL sheet with local-file browse"
  ```

---

### Task 7: Touch — Help/About info sheet

**Depends on:** Task 1 (`ModeView::Help { tab }`), Task 4 (`web/help-content.ts`'s `HELP_TEXT`/
`ABOUT_TEXT`/`KIND_LEGEND`). Touch currently has **no** Help/About surface at all — this task
adds one from scratch, following the exact pattern the file already uses for its other
mode-driven sheets (`renderFilterSheet` for `TypeFilter`, `renderConvertDialogShared` for
`Convert`, `renderPromptSheet` for `Prompt`, all called from `render()`, `web/touch/app.ts:382`).

**Files:**
- Modify: `web/touch/app.ts` (new info button + `renderHelpSheet` + open wiring + sheet div)
- Modify: `web/touch/style.css` (tab button styles)

**Interfaces:**
- Consumes: `snap.mode` as `{ Help: { tab: "Help" | "About" } }` (Task 1), `HELP_TEXT`/
  `ABOUT_TEXT`/`KIND_LEGEND` (Task 4's `web/help-content.ts`), `send(intent)` — touch's existing
  dispatch helper (already used throughout `web/touch/app.ts`, e.g. `send("EnterTypeFilter")` in
  `installShellHandlers`), `modeTag(m: ModeView): string` (already defined at
  `web/touch/app.ts:185`), `esc`/`IC` (already imported at `web/touch/app.ts:42` from
  `./render.js`), `sheets: Record<string, HTMLElement>` (already declared at
  `web/touch/app.ts:178`), `openSheet(name: string)`/`closeSheets()` (already used by every other
  sheet in this file).
- Produces: nothing new consumed elsewhere.

- [ ] **Step 1: Import `help-content.ts` and add the header info button to `appHTML()`**

  Add near the top of `web/touch/app.ts`, with the other local imports (line ~44-46):
  ```ts
  import { HELP_TEXT, ABOUT_TEXT, KIND_LEGEND } from "../help-content.js";
  ```

  In `appHTML()` (around line 274-277), add a button in the `edit-grp` group, after the theme
  button:
  ```ts
    `<button class="icon-btn" data-act="undo" title="Undo">${TIC.undo}</button>` +
    `<button class="icon-btn" data-act="redo" title="Redo">${TIC.redo}</button>` +
    `<button class="icon-btn" data-act="theme" title="Toggle theme">${TIC.theme}</button>` +
    `<button class="icon-btn" data-act="info" title="Help / About">${TIC.info}</button>` +
    "</div>" +
  ```
  Add the matching icon to the `TIC` object (near the other icon defs, around line 248-253):
  ```ts
    info: '<svg class="ic" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="9"/><path d="M12 11v6"/><path d="M12 7.5h.01"/></svg>',
  ```

  Add the sheet's root div in `appHTML()`, next to the other on-demand sheets (around
  line 328-330, right before the `url-sheet` div):
  ```ts
    '<div class="sheet ext-sheet"></div>' +
    // Help/About sheet (info button) — built on demand by `renderHelpSheet`.
    '<div class="sheet help-sheet"></div>' +
    // Open-from-URL sheet (More ▸ Open from URL) — built on demand by `openOpenSheet`.
    '<div class="sheet url-sheet"></div>' +
  ```

  Register it where the other `sheets.*` lookups happen (around line 1346-1353), next to
  `sheets.url`:
  ```ts
    sheets.ext = app.querySelector(".ext-sheet")!;
    sheets.help = app.querySelector(".help-sheet")!;
    sheets.url = app.querySelector(".url-sheet")!;
  ```

- [ ] **Step 2: Add the `renderHelpSheet` function**

  Add near `openOpenSheet` (Task 6). This mirrors `renderFilterSheet`'s "read from `snap.mode`,
  re-render on every snapshot" pattern rather than a fire-once `open*Sheet` function, since the
  tab flips via `send("ToggleHelpTab")` and must re-render live:
  ```ts
  function renderHelpSheet() {
    const tag = modeTag(snap!.mode);
    if (tag !== "Help") {
      if (sheets.help.classList.contains("open")) closeSheets();
      return;
    }
    const activeTab = (snap!.mode as { Help: { tab: "Help" | "About" } }).Help.tab;
    const legend = KIND_LEGEND[snap!.doc_format] ?? "";
    const body = activeTab === "Help" ? HELP_TEXT + "\n" + legend : ABOUT_TEXT;
    sheets.help.innerHTML =
      '<div class="grab"></div>' +
      `<div class="sheet-head"><h3>Help / About</h3><button class="close" data-act="closesheet">${IC.close}</button></div>` +
      '<div class="sheet-body">' +
      '<div class="help-tabs">' +
      `<button class="btn tab-btn${activeTab === "Help" ? " primary" : ""}" data-tab="Help">Help</button>` +
      `<button class="btn tab-btn${activeTab === "About" ? " primary" : ""}" data-tab="About">About</button>` +
      "</div>" +
      `<pre>${esc(body)}</pre>` +
      "</div>";
    sheets.help.querySelectorAll<HTMLElement>("[data-tab]").forEach((btn) => {
      btn.onclick = () => send("ToggleHelpTab");
    });
    if (!sheets.help.classList.contains("open")) openSheet("help");
  }
  ```

- [ ] **Step 3: Wire the info button and call `renderHelpSheet` from `render()`**

  In `installShellHandlers`'s switch (same block edited in Task 6 Step 2, `web/touch/app.ts`
  around line 1174-1179), add:
  ```ts
        case "info":
          send("EnterHelp");
          break;
  ```

  In `render()` (`web/touch/app.ts:382`), add a call alongside the existing per-mode sheet
  renders (next to the `tag === "Prompt"` block shown at the end of the excerpt read earlier):
  ```ts
    if (tag === "Prompt") renderPromptSheet((snap.mode as { Prompt: { kind: PromptView } }).Prompt.kind);
    else sheets.prompt.classList.remove("open");
    renderHelpSheet();
  ```

- [ ] **Step 4: Add tab-button CSS**

  In `web/touch/style.css`, add:
  ```css
  .help-tabs { display: flex; gap: 8px; margin-bottom: 10px; }
  ```

- [ ] **Step 5: `tsc --noEmit`, rebuild, manual verify**

  Run: `cd web && npx tsc --noEmit` — expected: no errors.
  Rebuild per Task 2 Step 3's scratchpad procedure. With device emulation (coarse pointer),
  tap the new info button — the Help/About sheet opens on the Help tab; tap "About" — shows
  version/author/license/GitHub; tap "Help" — back to keybindings; tap the close button.
  Report actual verification to the user.

- [ ] **Step 6: Commit**

  ```bash
  git add web/touch/app.ts web/touch/style.css
  git commit -m "feat(touch): add Help/About info sheet"
  ```

---

## Final integration check

- [ ] Run the full workspace gate once more after all 7 tasks: `cargo build && cargo test &&
  cargo clippy -- -D warnings && cargo fmt --check`.
- [ ] Per the `rebuild-wasm-web-after-core-change` memory: rebuild the wasm package
  (`wasm-pack build --target web` in `crates/confy-ffi`, from the scratchpad copy) and run
  `node functional_smoke.mjs` — expected: all checks pass (the `Intent`/`ModeView` wire contract
  changed in Task 1, so this must be re-verified, not assumed).
- [ ] Ask the user to do one end-to-end manual pass on desktop browser, touch/emulated browser,
  and the TUI binary before considering this feature complete.
