✅ **Shipped — historical reference.** See `CHANGELOG.md` for current behavior; this plan is kept for context, not as a live task list.

# Message System Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace confy-core's two-bucket `status`/`error` message model with a typed `Notice` (severity + source + text), consolidate prompt-question text into the wire's `ModeView::Prompt`, unify host toast/status rendering across TUI/web/touch, route CLI strings through the i18n catalog, and add an in-Session diagnostics ring — landing in six independently-green phases.

**Architecture:** Core owns one `Notice` model with severity derived from a `key -> Severity` table (`notice.rs`), never spelled at call sites; a five-kind `DiagEvent` ring (`diag.rs`) taps every notice plus dispatch/mutation/schema/convert events. `SessionSnapshot` dual-writes the new `notice` field alongside legacy `status`/`error` in Phase 1 so old web JS keeps working, then Phase 3 switches web to `notice` and deletes the legacy fields in the same commit. TUI, web, and CLI each get their own phase; diagnostics exports and docs land last.

**Tech Stack:** Rust (confy-core, confy-tui), TypeScript (web/, editors/vscode via the same web bundle), wasm/FFI boundary, JSON i18n catalogs (`i18n/en.json`, `i18n/zh-TW.json`).

**Spec:** `docs/superpowers/specs/2026-08-21-message-system-design.md` (the plan argues from this spec — executors MUST read both; exhaustive data tables — the 45-key severity mapping in §2.2, the 38 touch-toast call sites in §5.2/§12 fact verifications — are **not** duplicated here and must be read from the spec).

## Global Constraints

- Every phase compiles and its test suite passes green, in one commit each (§8). Never leave a phase half-done across a commit boundary.
- Zero new dependencies. No `tracing`/`log` crate (ADR 0008). `crates/confy-core/tests/no_fs_gate.rs` must stay green — no filesystem access in confy-core.
- Severity is **never** spelled at a core call site after Phase 1: sites call `Notice::core(key, args)` and `severity_of(key)` resolves the tier. This applies to host notices too — a host message with no catalog key cannot migrate (§2, §12 Q1/Q5).
- `dispatch(&mut self, intent: Intent) -> SessionSnapshot` (`dispatch.rs:304`) stays the sole mutation path. No new `set_*` methods on `Session`; host notices are `Intent::SetHostNotice { key, args, source }` (§12 Q6).
- `NoticeSource` is developer-facing only — rides the wire and feeds the diag ring, never rendered to the user (§12 Q2).
- All new/changed English strings get a matching `zh-TW` entry in the same commit (mirror test enforces, §10).
- `Notice`/`DiagEvent`/`Severity`/`NoticeSource`/`DiagLevel` derive `Debug, Clone[, Copy], PartialEq, Eq[, Serialize, Deserialize]` exactly as specified in §2 and §7 — copy the derive lists verbatim, don't add or drop traits.

---

## File Structure

| File | Responsibility |
|---|---|
| `crates/confy-core/src/session/notice.rs` (new) | `Severity`, `NoticeSource`, `Notice`, `severity_of(key) -> Severity` table, `Notice::core(key, args)` / `Notice::host_tui(key, args)` / `Notice::host_web(key, args)` constructors |
| `crates/confy-core/src/session/diag.rs` (new) | `DiagLevel`, `DiagEvent`, ring push helpers, the one notice-tap function |
| `crates/confy-core/src/session/session.rs` | `Session.notice: Option<Notice>` replaces `status`/`error`; `Session.diag: VecDeque<DiagEvent>`; `set_lang` clears `notice` |
| `crates/confy-core/src/session/view.rs` | `SessionSnapshot.notice` (new), `status`/`error` become computed dual-write fields in Phase 1, deleted in Phase 3; `ModeView::Prompt.question: String` (new); `rows[].has_descendant_violation` rename (Phase 3) |
| `crates/confy-core/src/session/state.rs` | No struct changes — `PromptKind` variants already carry their interpolation args |
| `crates/confy-core/src/session/intent.rs` | `Intent::SetHostNotice { key: &'static str, args: Vec<String>, source: NoticeSource }` (new variant) |
| `crates/confy-core/src/session/dispatch.rs` | `SetHostNotice` handler; diag `dispatch`/`mutation` recording |
| every core call site that sets `status`/`error` today (`clipboard.rs`, `inline_edit.rs`, `undo_redo.rs`, `session.rs`, `insertion.rs`, `type_filter.rs`, `schema_hint.rs`) | re-tier to `Notice::core(key, args)` |
| `crates/confy-core/src/session/i18n.rs` | no signature change; `tr`/`tr_args` reused by `notice.rs` and `cli.rs` |
| `i18n/en.json`, `i18n/zh-TW.json` | `core.prompt.*` (5 keys replacing `core.paste.collision` etc.), `core.schema.count`, `core.hint.enum`, `core.hint.bounded`, `tui.prompt.<kind>.legend` (renamed), `cli.*` (~10 keys), plus new catalog keys picked up by the 21 migrating touch messages |
| `crates/confy-tui/src/ui.rs` | `draw_status` severity coloring + priority order; `draw_prompt_overlay` reads `PromptView.question` + `.legend`; `~` overlay |
| `crates/confy-tui/src/overlay_diag.rs` (new) | read-only diag ring overlay |
| `crates/confy-tui/src/app.rs` | 11 pub-field write sites → `Intent::SetHostNotice` dispatches |
| `crates/confy-tui/src/keys.rs` | bind `~` to open the diag overlay |
| `web/types.ts` | mirror `Notice`/`Severity`/`NoticeSource`/`ModeView::Prompt.question` |
| `web/ui.ts` | desktop `#toast` element + severity→surface rendering; `?diag=1` drain |
| `web/touch/app.ts` | 38 `toast(` sites: 17 deleted, 7 → host notice dispatch, 14 migrated with new catalog keys |
| `web/prompt.ts` | delete `PROMPT_QUESTIONS`, `promptQuestion()`, `web.prompt.confirmFallback` |
| `web/panel.ts` | `schemaHintText` switches to `core.hint.enum`/`core.hint.bounded` |
| `web/i18n.ts` | no signature change |
| `crates/confy-tui/src/cli.rs` (or wherever the `convert` CLI subcommand lives) | render all strings via `tr`/`tr_args`; load config file for `--lang` resolution |
| `crates/confy-ffi` (wasm crate, exact path TBD by Task 15 discovery) | `diag_log() -> JsValue` export |
| `CONTEXT.md`, `docs/TUI.md`, `docs/WEBUI.md`, `CLAUDE.md`, `CHANGELOG.md` | Phase 6 docs pass |

---

## Task 1: `Severity` / `NoticeSource` / `Notice` model + `severity_of` table

**Files:**
- Create: `crates/confy-core/src/session/notice.rs`
- Modify: `crates/confy-core/src/session/mod.rs:1-18` (add `pub mod notice;` alphabetically after `intent`), `:19-40` (add `pub use notice::{Notice, NoticeSource, Severity};`)
- Test: `crates/confy-core/src/session/notice.rs` (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `crate::session::i18n::{tr, tr_args, Lang}` (existing, `i18n.rs:71,86`)
- Produces: `Severity::{Info,Success,Warn,Error}`, `NoticeSource::{Core,HostTui,HostWeb}`, `Notice{severity,text,source}`, `severity_of(key: &str) -> Severity`, `Notice::core(lang: Lang, key: &str, args: &[&str]) -> Notice`, `Notice::host_tui(lang: Lang, key: &str, args: &[&str]) -> Notice`, `Notice::host_web(lang: Lang, key: &str, args: &[&str]) -> Notice` — all three constructors call `severity_of(key)` internally (no explicit-severity path exists, per Global Constraints).

- [ ] **Step 1: Write the failing test for `severity_of` coverage**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_of_covers_the_full_catalog_table() {
        // One assertion per §2.2 row. Keep this table byte-identical to the
        // spec's §2.2 groups — it IS the single source of truth once Task 4
        // deletes the inline severities at call sites.
        let cases: &[(&str, Severity)] = &[
            ("core.error.generic", Severity::Error),
            ("core.add.error", Severity::Error),
            ("core.delete.error", Severity::Error),
            ("core.paste.error", Severity::Error),
            ("core.paste.comment-illegal", Severity::Error),
            ("core.remark.error", Severity::Error),
            ("core.rename.failed", Severity::Error),
            ("core.trailing.update-failed", Severity::Error),
            ("core.undo.error", Severity::Error),
            ("core.redo.error", Severity::Error),
            ("core.kind-switch.error", Severity::Error),
            ("core.readonly", Severity::Warn),
            ("core.clipboard.action-locked", Severity::Warn),
            ("core.comment.unsupported", Severity::Warn),
            ("core.trailing.inline-unsupported", Severity::Warn),
            ("core.reveal.hidden-by-filter", Severity::Warn),
            ("core.move.self", Severity::Warn),
            ("core.insert.collision", Severity::Warn),
            ("core.rename.empty-key", Severity::Warn),
            ("core.value.invalid", Severity::Warn),
            ("core.comment.invalid", Severity::Warn),
            ("core.fragment.invalid", Severity::Warn),
            ("core.remark.invalid", Severity::Warn),
            ("core.convert.root-only", Severity::Warn),
            ("core.kind-switch.unsupported", Severity::Warn),
            ("core.save.saved", Severity::Success),
            ("core.kind-switch.converted", Severity::Success),
            ("core.kind-switch.converted-generic", Severity::Success),
            ("core.clipboard.cut", Severity::Success),
            ("core.clipboard.copied", Severity::Success),
            ("core.clipboard.cut-changed", Severity::Success),
            ("core.clipboard.copied-changed", Severity::Success),
            ("core.save.nothing", Severity::Info),
            ("core.clipboard.empty", Severity::Info),
            ("core.clipboard.cleared", Severity::Info),
            ("core.selection.cleared", Severity::Info),
            ("core.undo.empty", Severity::Info),
            ("core.redo.empty", Severity::Info),
            ("core.paste.cancelled", Severity::Info),
            ("core.add.placeholder", Severity::Info),
            ("core.convert.aborted", Severity::Info),
        ];
        assert_eq!(cases.len(), 39, "39 non-prompt keys in §2.2 (11+14+7+9-2 renamed-away); update this count if §2.2 changes");
        for (key, expected) in cases {
            assert_eq!(severity_of(key), *expected, "key {key} classified wrong");
        }
    }

    #[test]
    fn notice_core_derives_severity_from_key() {
        let n = Notice::core(Lang::En, "core.save.saved", &[]);
        assert_eq!(n.severity, Severity::Success);
        assert_eq!(n.source, NoticeSource::Core);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p confy-core notice:: -- --nocapture`
Expected: FAIL — `notice` module does not exist yet.

- [ ] **Step 3: Write the implementation**

```rust
//! Notice model — the single-slot, user-facing transient message. See
//! `CONTEXT.md` § Messages & diagnostics.

use super::i18n::{tr_args, Lang};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Success,
    Warn,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NoticeSource {
    Core,
    HostTui,
    HostWeb,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Notice {
    pub severity: Severity,
    pub text: String,
    pub source: NoticeSource,
}

impl Notice {
    pub fn core(lang: Lang, key: &str, args: &[&str]) -> Self {
        Notice { severity: severity_of(key), text: tr_args(lang, key, args), source: NoticeSource::Core }
    }
    pub fn host_tui(lang: Lang, key: &str, args: &[&str]) -> Self {
        Notice { severity: severity_of(key), text: tr_args(lang, key, args), source: NoticeSource::HostTui }
    }
    pub fn host_web(lang: Lang, key: &str, args: &[&str]) -> Self {
        Notice { severity: severity_of(key), text: tr_args(lang, key, args), source: NoticeSource::HostWeb }
    }
}

/// Single source of truth for a catalog key's tier (§2.2 of the design spec).
/// Every `core.*`/host-notice key MUST appear here before it can be used in
/// a `Notice::*` constructor — there is no explicit-severity escape hatch.
pub fn severity_of(key: &str) -> Severity {
    match key {
        "core.error.generic" | "core.add.error" | "core.delete.error" | "core.paste.error"
        | "core.paste.comment-illegal" | "core.remark.error" | "core.rename.failed"
        | "core.trailing.update-failed" | "core.undo.error" | "core.redo.error"
        | "core.kind-switch.error" => Severity::Error,

        "core.readonly" | "core.clipboard.action-locked" | "core.comment.unsupported"
        | "core.trailing.inline-unsupported" | "core.reveal.hidden-by-filter" | "core.move.self"
        | "core.insert.collision" | "core.rename.empty-key" | "core.value.invalid"
        | "core.comment.invalid" | "core.fragment.invalid" | "core.remark.invalid"
        | "core.convert.root-only" | "core.kind-switch.unsupported" => Severity::Warn,

        "core.save.saved" | "core.kind-switch.converted" | "core.kind-switch.converted-generic"
        | "core.clipboard.cut" | "core.clipboard.copied" | "core.clipboard.cut-changed"
        | "core.clipboard.copied-changed" => Severity::Success,

        "core.save.nothing" | "core.clipboard.empty" | "core.clipboard.cleared"
        | "core.selection.cleared" | "core.undo.empty" | "core.redo.empty"
        | "core.paste.cancelled" | "core.add.placeholder" | "core.convert.aborted" => Severity::Info,

        _ => panic!("severity_of: unmapped notice key {key:?} — add it to the table in notice.rs"),
    }
}
```

Note: Task 9 (Web host messages) and later touch-migration tasks add their own keys to this `match` — each addition needs its own `severity_of` arm plus a catalog entry, never an inline severity.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p confy-core notice::`
Expected: PASS (both tests).

- [ ] **Step 5: Commit**

```bash
git add crates/confy-core/src/session/notice.rs crates/confy-core/src/session/mod.rs
git commit -m "feat(core): add Notice/Severity/NoticeSource model with severity_of table"
```

---

## Task 2: `DiagLevel`/`DiagEvent` ring + notice tap

**Files:**
- Create: `crates/confy-core/src/session/diag.rs`
- Modify: `crates/confy-core/src/session/mod.rs` (add `pub mod diag;`, `pub use diag::{DiagEvent, DiagLevel};`)
- Test: `crates/confy-core/src/session/diag.rs` (inline)

**Interfaces:**
- Consumes: nothing external.
- Produces: `DiagLevel::{Debug,Info,Warn,Error}`, `DiagEvent{seq,level,kind,detail}`, `DiagRing` struct wrapping `VecDeque<DiagEvent>` (capacity 256) with `push(&mut self, level, kind, detail)` and `iter(&self) -> impl Iterator<Item=&DiagEvent>`. Task 3 wires `Session.diag: DiagRing`.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_evicts_oldest_past_256() {
        let mut ring = DiagRing::default();
        for i in 0..260 {
            ring.push(DiagLevel::Debug, "dispatch", format!("intent={i}"));
        }
        let events: Vec<_> = ring.iter().collect();
        assert_eq!(events.len(), 256);
        assert_eq!(events.first().unwrap().detail, "intent=4"); // oldest 4 evicted
        assert_eq!(events.last().unwrap().detail, "intent=259");
    }

    #[test]
    fn seq_is_monotonic() {
        let mut ring = DiagRing::default();
        ring.push(DiagLevel::Info, "notice", "a".into());
        ring.push(DiagLevel::Info, "notice", "b".into());
        let events: Vec<_> = ring.iter().collect();
        assert_eq!(events[1].seq, events[0].seq + 1);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p confy-core diag:: -- --nocapture`
Expected: FAIL — module does not exist.

- [ ] **Step 3: Write the implementation**

```rust
//! Diagnostics ring — see ADR 0008 and design spec §7. Developer-facing,
//! English-only, no i18n. Zero new dependencies (no `tracing`/`log`).

use std::collections::VecDeque;

const CAPACITY: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagLevel {
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone)]
pub struct DiagEvent {
    pub seq: u64,
    pub level: DiagLevel,
    pub kind: &'static str, // "dispatch" | "mutation" | "schema" | "convert" | "notice"
    pub detail: String,
}

#[derive(Debug, Default)]
pub struct DiagRing {
    events: VecDeque<DiagEvent>,
    next_seq: u64,
}

impl DiagRing {
    pub fn push(&mut self, level: DiagLevel, kind: &'static str, detail: String) {
        if self.events.len() == CAPACITY {
            self.events.pop_front();
        }
        self.events.push_back(DiagEvent { seq: self.next_seq, level, kind, detail });
        self.next_seq += 1;
    }

    pub fn iter(&self) -> impl Iterator<Item = &DiagEvent> {
        self.events.iter()
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p confy-core diag::`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/confy-core/src/session/diag.rs crates/confy-core/src/session/mod.rs
git commit -m "feat(core): add DiagEvent ring buffer (capacity 256)"
```

---

## Task 3: Wire `Session.notice`/`Session.diag`, notice tap, `SetLang` clear

**Files:**
- Modify: `crates/confy-core/src/session/session.rs:19-55` (struct fields), `:111` area (`set_lang`)
- Test: `crates/confy-core/tests/session_headless.rs` (new test function appended)

**Interfaces:**
- Consumes: `Notice`, `NoticeSource`, `Severity` (Task 1), `DiagRing`, `DiagLevel` (Task 2)
- Produces: `Session.notice: Option<Notice>`, `Session.diag: DiagRing`, `Session::set_notice(&mut self, notice: Notice)` — the ONE tap point (records a `"notice"` diag event with `severity=.., source=.., key=.., text=..` before storing). All later tasks that set a notice MUST go through `set_notice`, never assign `self.notice` directly outside `session.rs`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn set_notice_taps_the_diag_ring() {
    let mut session = Session::new_empty(); // adjust to actual empty-session constructor used elsewhere in this file
    session.set_notice(Notice::core(Lang::En, "core.save.saved", &[]));
    let last = session.diag.iter().last().expect("diag event recorded");
    assert_eq!(last.kind, "notice");
    assert!(last.detail.contains("core.save.saved"));
    assert_eq!(session.notice.as_ref().unwrap().severity, Severity::Success);
}

#[test]
fn set_lang_clears_notice() {
    let mut session = Session::new_empty();
    session.set_notice(Notice::core(Lang::En, "core.save.saved", &[]));
    session.dispatch(Intent::SetLang("zh-TW".into())); // adjust to actual SetLang intent shape
    assert!(session.notice.is_none());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p confy-core --test session_headless set_notice -- --nocapture`
Expected: FAIL — `notice`/`diag` fields and `set_notice` don't exist; `set_lang` doesn't clear.

- [ ] **Step 3: Write the implementation**

In `session.rs`, replace:
```rust
pub status: Option<String>,
pub error: Option<String>,
```
with:
```rust
pub notice: Option<Notice>,
pub diag: crate::session::diag::DiagRing,
```
Add:
```rust
impl Session {
    /// Sole write path for `notice` — every core/host notice assignment
    /// goes through here so the diag ring sees "what did the user see, in
    /// order" for every notice, not just host ones (design spec §7, §12 Q3).
    pub fn set_notice(&mut self, notice: Notice) {
        self.diag.push(
            crate::session::diag::DiagLevel::Info,
            "notice",
            format!("severity={:?} source={:?} text={:?}", notice.severity, notice.source, notice.text),
        );
        self.notice = Some(notice);
    }
}
```
In `set_lang` (currently `session.rs:111`, assigns `self.lang` only):
```rust
pub fn set_lang(&mut self, lang: Lang) {
    self.lang = lang;
    self.notice = None; // §12: SetLang clears — new behavior, prevents stale-language text
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p confy-core --test session_headless set_notice set_lang_clears`
Expected: PASS. Also run `cargo build -p confy-core` and fix every compile error from the removed `status`/`error` fields — at this point they will be widespread (call sites, `Default`/constructor impls); leave those as `todo!()` stubs ONLY if Task 4 immediately follows in the same session, otherwise this step is not shippable alone. Prefer doing Task 3 and Task 4 back to back without an intermediate commit if the compiler forces it — see the note after Step 5.

- [ ] **Step 5: Commit**

```bash
git add crates/confy-core/src/session/session.rs
git commit -m "feat(core): replace Session status/error with notice + diag ring, wire SetLang clear"
```

**Note:** if removing `status`/`error` from `Session` breaks compilation across many files before Task 4 re-tiers them, do Tasks 3 and 4 as one combined commit instead — the Global Constraint is "every phase compiles green", not every task; task boundaries inside Phase 1 may need folding here. Use judgment; don't force a red intermediate commit.

---

## Task 4: Re-tier every core call site to `Notice::core(key, args)`

**Files:**
- Modify: every file under `crates/confy-core/src/session/` that assigns `self.status = ...` / `self.error = ...` today: `clipboard.rs`, `inline_edit.rs`, `undo_redo.rs`, `session.rs`, `insertion.rs`, `type_filter.rs`, `schema_hint.rs`, and any others `cargo build` surfaces.
- Test: existing `crates/confy-core/tests/session_headless.rs`, `schema_headless.rs` (updated in Task 6, not here — this task only changes production code)

**Interfaces:**
- Consumes: `Session::set_notice` (Task 3), `Notice::core` (Task 1)
- Produces: no new public API; internal call sites only.

- [ ] **Step 1: Find every site**

Run: `grep -rn 'self\.status = \|self\.error = ' crates/confy-core/src/session/`
Expected: a list of every site to convert — this IS the "find failing usages" step for a mechanical refactor task (no new test to write first; Task 3's compile break is the red state).

- [ ] **Step 2: Convert every site mechanically**

For each site, replace the pattern
```rust
self.status = Some(tr_args(self.lang, "core.save.saved", &[]));
self.error = None;
```
with
```rust
self.set_notice(Notice::core(self.lang, "core.save.saved", &[]));
```
and every site that only clears (`self.status = None; self.error = None;`) becomes `self.notice = None;`. Apply this identically to all 39 non-prompt keys and their call sites — the exact key list is `notice.rs`'s `severity_of` table from Task 1; do not invent new keys here. For the 5 prompt keys (`core.prompt.*`), see Task 5 — do NOT convert prompt-setting sites in this task, they move to `ModeView::Prompt.question` instead of `Notice`.

Also delete the today-duplicated pattern noted in spec §2 (`clipboard.rs:263-264`): a prompt-opening site that writes `status`/`error` *and* opens the prompt — after Task 5 the prompt-opening path sets ONLY the mode; delete the redundant notice write here, in this task, since it's a `status`/`error` site being converted anyway.

- [ ] **Step 3: Build to confirm no leftover raw field references**

Run: `cargo build -p confy-core`
Expected: builds clean (aside from expected breakage in `view.rs`/tests not yet updated — those are Tasks 5-6; if `cargo build -p confy-core` as a whole crate fails only in `view.rs`/`dispatch.rs`/tests, that's expected here and resolved by Task 5/6. If it fails inside `session/*.rs` production files, that's this task's bug — fix it.)

- [ ] **Step 4: Run existing core tests (expect remaining red only from view.rs/snapshot, not from these sites)**

Run: `cargo test -p confy-core --lib`
Expected: compiles; failures (if any) trace to `SessionSnapshot` still expecting `status`/`error` — that's Task 6, not this task.

- [ ] **Step 5: Commit**

```bash
git add crates/confy-core/src/session/
git commit -m "refactor(core): re-tier all status/error call sites to Notice::core via severity_of"
```

---

## Task 5: Prompt question consolidation (`ModeView::Prompt.question`, catalog rename)

**Files:**
- Modify: `crates/confy-core/src/session/view.rs` (`ModeView::Prompt` variant, `prompt_view()` rendering function), `i18n/en.json`, `i18n/zh-TW.json` (rename `core.paste.collision`→`core.prompt.collision`, `core.quit.confirm`→`core.prompt.confirm-quit`, `core.type-change`→`core.prompt.type-change`, `core.paste.array-upgrade-confirm`→`core.prompt.array-upgrade`, add new `core.prompt.jsonc-upgrade`; strip embedded `? y/n` / `— o/r/c` legends from all five strings)
- Test: `crates/confy-core/tests/session_headless.rs`

**Interfaces:**
- Consumes: `PromptKind::{Collision{key},ConfirmQuit,TypeChange{from,to},ArrayUpgrade{target,on_collision},JsoncUpgrade{pending}}` (existing, `state.rs:154-170`), `tr_args`
- Produces: `ModeView::Prompt { kind: PromptKindView, question: String }` — `question` computed inside `prompt_view()` from `PromptKind` + `Session.lang`, deterministic per snapshot.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn prompt_question_renders_from_kind_not_status() {
    let mut session = /* build a session, trigger a Collision prompt */;
    let snap = session.dispatch(/* the intent that opens Collision prompt for key "port" */);
    match snap.mode {
        ModeView::Prompt { question, .. } => {
            assert!(question.contains("port"));
            assert!(snap.status.is_none()); // dual-write still populates status/error (Task 6), but never with prompt text
        }
        other => panic!("expected Prompt mode, got {other:?}"),
    }
}

#[test]
fn prompt_question_rerenders_on_language_switch() {
    let mut session = /* ...trigger Collision prompt in En... */;
    session.dispatch(Intent::SetLang("zh-TW".into()));
    let snap = session.snapshot(); // or whatever recomputes the view
    match snap.mode {
        ModeView::Prompt { question, .. } => assert!(question.contains("鍵") /* actual zh-TW substring */),
        other => panic!("expected Prompt mode, got {other:?}"),
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p confy-core --test session_headless prompt_question`
Expected: FAIL — `ModeView::Prompt` has no `question` field yet.

- [ ] **Step 3: Implement**

Add `question: String` to `ModeView::Prompt`. In `prompt_view()` (or equivalent view-construction function in `view.rs`), match on `Mode::Prompt(kind)` and render:
```rust
fn prompt_question(lang: Lang, kind: &PromptKind) -> String {
    match kind {
        PromptKind::Collision { key } => tr_args(lang, "core.prompt.collision", &[key]),
        PromptKind::ConfirmQuit => tr_args(lang, "core.prompt.confirm-quit", &[]),
        PromptKind::TypeChange { from, to } => tr_args(lang, "core.prompt.type-change", &[from, to]),
        PromptKind::ArrayUpgrade { .. } => tr_args(lang, "core.prompt.array-upgrade", &[]),
        PromptKind::JsoncUpgrade { .. } => tr_args(lang, "core.prompt.jsonc-upgrade", &[]),
    }
}
```
Update `i18n/en.json`/`i18n/zh-TW.json`: rename the four keys, add the fifth, strip legends from all five string values (legends move to `tui.prompt.<kind>.legend` in Task 8).

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p confy-core --test session_headless prompt_question`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/confy-core/src/session/view.rs i18n/en.json i18n/zh-TW.json
git commit -m "feat(core): render prompt question per-snapshot from PromptKind, consolidate core.prompt.* keys"
```

---

## Task 6: `SessionSnapshot` dual-write, `Intent::SetHostNotice`, diag dispatch/mutation taps, test helpers, coverage tests

**Files:**
- Modify: `crates/confy-core/src/session/view.rs:212-256` (`SessionSnapshot`: add `notice: Option<Notice>`, keep `status`/`error` computed), `crates/confy-core/src/session/intent.rs` (new `SetHostNotice` variant), `crates/confy-core/src/session/dispatch.rs:304-317` (handler + diag `dispatch`/`mutation` recording)
- Test: `crates/confy-core/tests/session_headless.rs`, `crates/confy-core/tests/schema_headless.rs` (migrate the 82 Some/None assertions + 7 text assertions via new helpers)

**Interfaces:**
- Consumes: `Session.notice`, `Session.diag`, `Notice::host_tui`/`host_web` (Task 1), `set_notice` (Task 3)
- Produces: `SessionSnapshot::error_text(&self) -> Option<&str>` (`Some` iff `notice.severity == Error`), `SessionSnapshot::status_text(&self) -> Option<&str>` (`Some` iff notice present and severity != Error), `Intent::SetHostNotice { key: &'static str, args: Vec<String>, source: NoticeSource }`.

- [ ] **Step 1: Write the failing tests**

```rust
// session_headless.rs — slot-occupancy test (§12 Q7 follow-up)
#[test]
fn warn_notice_occupies_status_not_error() {
    let mut session = /* trigger a Warn, e.g. readonly guard */;
    let snap = session.dispatch(/* intent that hits core.readonly */);
    assert!(snap.error_text().is_none());
    assert!(snap.status_text().is_some());
    assert!(snap.notice.as_ref().unwrap().severity == Severity::Warn);
}

#[test]
fn error_notice_occupies_error_not_status() {
    let mut session = /* trigger an Error */;
    let snap = session.dispatch(/* intent that fails */);
    assert!(snap.error_text().is_some());
    assert!(snap.status_text().is_none());
}

#[test]
fn no_notice_is_both_none() {
    let mut session = /* fresh session, no notice set */;
    let snap = session.snapshot();
    assert!(snap.error_text().is_none());
    assert!(snap.status_text().is_none());
}

#[test]
fn set_host_notice_intent_goes_through_dispatch() {
    let mut session = /* fresh session */;
    let snap = session.dispatch(Intent::SetHostNotice {
        key: "core.clipboard.action-locked",
        args: vec![],
        source: NoticeSource::HostTui,
    });
    assert_eq!(snap.notice.as_ref().unwrap().source, NoticeSource::HostTui);
    assert_eq!(snap.notice.as_ref().unwrap().severity, Severity::Warn);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p confy-core --test session_headless warn_notice error_notice no_notice set_host_notice`
Expected: FAIL — `error_text`/`status_text`/`SetHostNotice` don't exist.

- [ ] **Step 3: Implement**

`view.rs` — add to `SessionSnapshot`:
```rust
pub notice: Option<Notice>,
```
Keep `status`/`error` as-is on the struct but compute them at snapshot-build time from `notice` (Global dual-write mapping, §10/§11 Q6):
```rust
let (status, error) = match &notice {
    Some(n) if n.severity == Severity::Error => (None, Some(n.text.clone())),
    Some(n) => (Some(n.text.clone()), None),
    None => (None, None),
};
```
Add accessor methods:
```rust
impl SessionSnapshot {
    pub fn error_text(&self) -> Option<&str> {
        self.notice.as_ref().filter(|n| n.severity == Severity::Error).map(|n| n.text.as_str())
    }
    pub fn status_text(&self) -> Option<&str> {
        self.notice.as_ref().filter(|n| n.severity != Severity::Error).map(|n| n.text.as_str())
    }
}
```
`intent.rs` — add variant (with a non-user-action comment per Global Constraints):
```rust
/// Not a user action — internal channel for hosts to report their own
/// errors/notices through the sole dispatch path (design spec §5, §12 Q6).
SetHostNotice { key: &'static str, args: Vec<String>, source: NoticeSource },
```
`dispatch.rs` — handler in `apply()`:
```rust
Intent::SetHostNotice { key, args, source } => {
    let args_ref: Vec<&str> = args.iter().map(String::as_str).collect();
    let notice = match source {
        NoticeSource::HostTui => Notice::host_tui(self.lang, key, &args_ref),
        NoticeSource::HostWeb => Notice::host_web(self.lang, key, &args_ref),
        NoticeSource::Core => unreachable!("hosts never claim NoticeSource::Core"),
    };
    self.set_notice(notice);
}
```
Add diag taps for `dispatch` (Debug, every intent, at the top of `dispatch()`) and `mutation` (Info/Error, variant name + ok/err, wherever `apply()`'s result is known) per spec §7 — one `self.diag.push(...)` call at each point.

- [ ] **Step 4: Migrate the 82 Some/None + 7 text assertions**

In `session_headless.rs`, `schema_headless.rs`, `app.rs` (TUI test file — cross-reference in Task 12 if TUI tests also assert `status`/`error` directly): replace `snap.error.is_none()` → `snap.error_text().is_none()`, `snap.status.is_some()` → `snap.status_text().is_some()`, and the 7 text-comparison sites (`session_headless.rs:787,798,812`, `schema_headless.rs:805`) → compare `snap.error_text()`/`snap.status_text()` against the same expected string. Preserve each assertion's original truth value exactly — this is a mechanical rename, not a behavior change.

- [ ] **Step 5: Run full core test suite**

Run: `cargo test -p confy-core`
Expected: PASS, including the new severity-table-coverage test (Task 1), slot-occupancy tests, and all migrated assertions.

- [ ] **Step 6: Commit**

```bash
git add crates/confy-core/
git commit -m "feat(core): SessionSnapshot.notice + dual-write, Intent::SetHostNotice, diag taps, migrate status/error test assertions"
```

**Phase 1 checkpoint:** `cargo test -p confy-core` green, `no_fs_gate.rs` green. This is the phase-1 commit boundary from spec §8 row 1 — Tasks 1-6 together satisfy it (see the note in Task 3 about not forcing a red intermediate commit).

---

## Task 7: TUI severity rendering + `draw_status` priority

**Files:**
- Modify: `crates/confy-tui/src/ui.rs` (`draw_status`, around existing lines `481-482` error-never-hidden invariant)
- Test: `crates/confy-tui` test file covering `draw_status` (find via `grep -rn 'fn draw_status' crates/confy-tui/src`; add a rendering-order test there, or a new `crates/confy-tui/tests/status_priority.rs` if `draw_status` isn't unit-testable directly — check whether it takes a `&SessionSnapshot` and returns styled text vs. writing to a terminal buffer, and test at that boundary)

**Interfaces:**
- Consumes: `SessionSnapshot.notice: Option<Notice>` (Task 6), `Severity`
- Produces: no new public API — internal rendering function change.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn error_notice_wins_over_active_input() {
    let snap = /* SessionSnapshot with notice=Error and an active Filter query */;
    let priority = status_line_source(&snap); // whatever draw_status's internal decision fn is named after refactor
    assert_eq!(priority, StatusSource::ErrorNotice);
}

#[test]
fn non_error_notice_demoted_below_active_input_but_not_cleared() {
    let mut snap = /* SessionSnapshot with notice=Warn and an active Filter query */;
    assert_eq!(status_line_source(&snap), StatusSource::ActiveInput);
    snap.mode = /* exit filter input */;
    assert_eq!(status_line_source(&snap), StatusSource::Notice); // Warn reappears
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p confy-tui status_line_source`
Expected: FAIL — function doesn't exist / priority not implemented yet.

- [ ] **Step 3: Implement**

Extract (or add, if `draw_status` is monolithic) a pure decision function:
```rust
enum StatusSource { ErrorNotice, ActiveInput, Notice, EditHint, None }

fn status_line_source(snap: &SessionSnapshot) -> StatusSource {
    if matches!(&snap.notice, Some(n) if n.severity == Severity::Error) {
        return StatusSource::ErrorNotice;
    }
    if is_active_input(&snap.mode) { // existing Filter-query / Edit-mode check
        return StatusSource::ActiveInput;
    }
    if snap.notice.is_some() {
        return StatusSource::Notice;
    }
    if has_edit_hint(&snap.mode) {
        return StatusSource::EditHint;
    }
    StatusSource::None
}
```
Wire severity → color: `Success` green, `Info` default, `Warn` yellow, `Error` red-bg/white-text (reuse existing error style). This is rendering-only — `snap.notice` is never mutated by this function.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p confy-tui status_line_source`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/confy-tui/src/ui.rs
git commit -m "feat(tui): severity-driven draw_status coloring and priority order"
```

---

## Task 8: TUI prompt overlay reads `PromptView.question` + legend rename

**Files:**
- Modify: `crates/confy-tui/src/ui.rs:652-673` (`draw_prompt_overlay` — currently re-renders locally via `tr_args`, per §12 fact verification), `i18n/en.json`, `i18n/zh-TW.json` (rename `tui.prompt.<kind>` → `tui.prompt.<kind>.legend`, strip to legend-only text e.g. `o:overwrite  r:rename  c:cancel`)
- Test: `crates/confy-tui` prompt overlay test (locate via `grep -rn 'draw_prompt_overlay' crates/confy-tui`)

**Interfaces:**
- Consumes: `ModeView::Prompt { kind, question }` (Task 5)
- Produces: no new public API.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn overlay_renders_question_from_snapshot_not_local_tr_args() {
    let snap = /* SessionSnapshot with ModeView::Prompt { kind: Collision, question: "..." } */;
    let lines = render_prompt_overlay_lines(&snap); // adjust name to actual testable seam
    assert!(lines[0].contains(&snap_question_text(&snap)));
}

#[test]
fn overlay_legend_line_is_separate_from_question() {
    let snap = /* Prompt with question text containing no "o/r/c" substring, since legends were stripped in Task 5 */;
    let lines = render_prompt_overlay_lines(&snap);
    assert!(!lines[0].contains("o:overwrite"));
    assert!(lines.last().unwrap().contains("o:overwrite"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p confy-tui overlay_renders_question`
Expected: FAIL — overlay still calls `tr_args` locally instead of reading `question`.

- [ ] **Step 3: Implement**

In `draw_prompt_overlay`, delete the local `tr_args(...)` reconstruction of the question line; use `snap.mode`'s `ModeView::Prompt { question, .. }` directly for line 1. Keep the 3-line dialog shape; append the legend line via the renamed `tui.prompt.<kind>.legend` key.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p confy-tui overlay_renders_question overlay_legend_line`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/confy-tui/src/ui.rs i18n/en.json i18n/zh-TW.json
git commit -m "feat(tui): prompt overlay consumes PromptView.question, rename tui.prompt.* to .legend"
```

---

## Task 9: TUI host error sites → `Intent::SetHostNotice`

**Files:**
- Modify: `crates/confy-tui/src/app.rs` (11 pub-field write sites: `377,385,454,483,520,525,539,556,678,687,694` — save/editor/convert-write paths), `crates/confy-tui/src/schema_io.rs` (fetch failure sites), config-write site (locate via `grep -rn 'session\.error =\|session\.status =' crates/confy-tui/src`)
- Test: `crates/confy-tui` integration test asserting no direct `session.error`/`session.status` writes remain

**Interfaces:**
- Consumes: `Intent::SetHostNotice { key, args, source: NoticeSource::HostTui }` (Task 6)
- Produces: no new public API. New host-error catalog keys as needed — each gets a `severity_of` entry (Task 1's table) and an `en.json`/`zh-TW.json` pair (e.g. `tui.host.save-failed`, `tui.host.schema-fetch-failed`, `tui.host.config-write-failed` — name per existing TUI key conventions found at the actual sites).

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn no_direct_status_error_writes_remain_in_app_rs() {
    let src = std::fs::read_to_string("src/app.rs").unwrap();
    assert!(!src.contains("self.session.error ="), "found a raw error write — migrate to Intent::SetHostNotice");
    assert!(!src.contains("self.session.status ="), "found a raw status write — migrate to Intent::SetHostNotice");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p confy-tui no_direct_status_error_writes`
Expected: FAIL — 11 sites remain.

- [ ] **Step 3: Convert each site**

Pattern:
```rust
// before
self.session.error = Some(format!("failed to save: {e}"));
// after
self.session.dispatch(Intent::SetHostNotice {
    key: "tui.host.save-failed",
    args: vec![e.to_string()],
    source: NoticeSource::HostTui,
});
```
Add each new key to `notice.rs`'s `severity_of` (Error tier for failures) and both catalogs. Apply to all 11 `app.rs` sites plus `schema_io.rs` fetch and the config-write site.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p confy-tui no_direct_status_error_writes`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/confy-tui/src/app.rs crates/confy-tui/src/schema_io.rs crates/confy-core/src/session/notice.rs i18n/en.json i18n/zh-TW.json
git commit -m "refactor(tui): host error sites dispatch Intent::SetHostNotice instead of raw field writes"
```

---

## Task 10: `core.schema.count` shared string (TUI footer)

**Files:**
- Modify: `crates/confy-tui/src/ui.rs` ("N schema warnings" footer site), `i18n/en.json`, `i18n/zh-TW.json` (new `core.schema.count`)
- Test: `crates/confy-tui` footer rendering test

**Interfaces:**
- Consumes: `tr_args`, `SessionSnapshot.schema_status.violation_count` (existing field)
- Produces: no new public API.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn footer_uses_shared_schema_count_key() {
    let snap = /* SessionSnapshot with schema_status.violation_count = 3 */;
    let footer = render_footer(&snap); // adjust to actual footer fn name
    assert_eq!(footer, tr_args(Lang::En, "core.schema.count", &["3"]));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p confy-tui footer_uses_shared_schema_count_key`
Expected: FAIL — key doesn't exist / footer still hand-rolls the string.

- [ ] **Step 3: Implement**

Add `"core.schema.count": "{0} schema warning(s)"` to `en.json`, matching zh-TW translation. Replace the TUI's hand-rolled format with `tr_args(lang, "core.schema.count", &[count.to_string().as_str()])`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p confy-tui footer_uses_shared_schema_count_key`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/confy-tui/src/ui.rs i18n/en.json i18n/zh-TW.json
git commit -m "feat(tui): schema-warning footer uses shared core.schema.count key"
```

---

## Task 11: `~` diag overlay in TUI

**Files:**
- Create: `crates/confy-tui/src/overlay_diag.rs`
- Modify: `crates/confy-tui/src/keys.rs` (bind `~`, confirmed unbound — bound punctuation today is space/`/`/`?`), `crates/confy-tui/src/app.rs` (mode/state to track overlay open), `crates/confy-tui/src/mod.rs` or equivalent (register new module)
- Test: `crates/confy-tui` overlay open/close test

**Interfaces:**
- Consumes: `Session.diag: DiagRing` (Task 3) — read-only, via `session.diag.iter()`
- Produces: `overlay_diag::render(diag: &DiagRing) -> Vec<String>` (or matches the Help popup's existing render signature — reuse its shape per spec §7).

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn tilde_opens_diag_overlay() {
    let mut app = /* fresh App */;
    app.handle_key(Key::Char('~'));
    assert!(app.diag_overlay_open);
}

#[test]
fn diag_overlay_shows_newest_event_at_bottom() {
    let mut app = /* App with a Session that has 3 diag events pushed */;
    app.handle_key(Key::Char('~'));
    let lines = overlay_diag::render(&app.session.diag);
    assert!(lines.last().unwrap().contains(/* the 3rd (newest) event's detail substring */));
}

#[test]
fn esc_closes_diag_overlay() {
    let mut app = /* App with overlay open */;
    app.handle_key(Key::Esc);
    assert!(!app.diag_overlay_open);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p confy-tui diag_overlay tilde_opens`
Expected: FAIL — overlay doesn't exist, `~` unbound.

- [ ] **Step 3: Implement**

`overlay_diag.rs`:
```rust
use crate::session::diag::DiagRing;

pub fn render(diag: &DiagRing) -> Vec<String> {
    diag.iter()
        .map(|e| format!("[{:?}] {} {}", e.level, e.kind, e.detail))
        .collect()
}
```
Bind `~` in `keys.rs` to toggle `app.diag_overlay_open`; wire `Esc` to close it when open (mirrors the existing Help popup's open/close handling — copy its structure, not its content).

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p confy-tui diag_overlay tilde_opens esc_closes`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/confy-tui/src/overlay_diag.rs crates/confy-tui/src/keys.rs crates/confy-tui/src/app.rs
git commit -m "feat(tui): ~ opens read-only diag ring overlay"
```

**Phase 2 checkpoint:** `cargo test -p confy-tui` green; manual TUI pass (open `~`, trigger a Warn/Error, confirm demotion-and-reappear behavior, confirm prompt legend line).

---

## Task 12: `web/types.ts` mirror + desktop `notice`/`question` consumption + `#toast`

**Files:**
- Modify: `web/types.ts` (mirror `Severity`, `NoticeSource`, `Notice`, `ModeView.Prompt.question`), `web/ui.ts` (severity→surface rendering, new `#toast` element wiring), `web/index.html` (add `#toast` element)
- Test: `web/render.spec.mjs`, `web/functional_smoke.mjs`

**Interfaces:**
- Consumes: `SessionSnapshot.notice` (wasm FFI, mirrors Task 6's Rust type)
- Produces: `toast(text: string, durationMs?: number)` in `ui.ts` — desktop's own toast, structurally identical to `touch/app.ts::toast()`, reused (not reimplemented) if practical — check whether extracting a shared `web/toast.ts` is cheaper than duplicating; if so add `web/toast.ts` and have both `ui.ts` and `touch/app.ts` import it (Task 13 will then update the import instead of `touch/app.ts`'s own copy).

- [ ] **Step 1: Write the failing test**

```js
// web/render.spec.mjs
test("Success notice shows toast and status bar text", () => {
  const snap = { ...baseSnapshot, notice: { severity: "success", text: "Saved", source: "core" } };
  render(snap);
  assert.equal(document.querySelector("#toast").textContent, "Saved");
  assert.equal(document.querySelector("#status").textContent, "Saved");
});

test("Error notice shows red status bar, no toast", () => {
  const snap = { ...baseSnapshot, notice: { severity: "error", text: "Failed", source: "core" } };
  render(snap);
  assert.equal(document.querySelector("#toast").textContent, "");
  assert.ok(document.querySelector("#status").classList.contains("sev-error"));
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `node --test web/render.spec.mjs`
Expected: FAIL — `#toast` doesn't exist, `types.ts` has no `notice`.

- [ ] **Step 3: Implement**

`types.ts`:
```ts
export type Severity = "info" | "success" | "warn" | "error";
export type NoticeSource = "core" | "host-tui" | "host-web";
export interface Notice { severity: Severity; text: string; source: NoticeSource; }
```
Add `notice: Notice | null` to `SessionSnapshot`; add `question: string` to the `Prompt` mode variant.
`index.html`: add `<div id="toast" class="hidden"></div>`.
`ui.ts`: one severity→surface function per §5.2's table (Success⇒toast+status, Info/Warn⇒status only with `.sev-warn` tint, Error⇒status red `.sev-error` + click-to-clear), called from the render loop whenever `snap.notice` changes.

- [ ] **Step 4: Run test to verify it passes**

Run: `node --test web/render.spec.mjs`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add web/types.ts web/ui.ts web/index.html
git commit -m "feat(web): types.ts mirrors Notice, desktop gets #toast + severity-driven surfaces"
```

---

## Task 13: Touch toast migration (17 delete / 7 host-notice / 14 migrate)

**Files:**
- Modify: `web/touch/app.ts` (all 38 `toast(` call sites — read the exact breakdown from spec §5.2/§12 fact verifications before starting: 24 clipboard-locked sites at `app.ts:695,726,761,888,1047,1231,1549` + keyboard twins `1536,1556,1580` are the 7 host-op guards; the remaining 17 clipboard-locked sites are pure duplicates of core's `guard_clipboard_locked` and are deleted outright; the other 14 are genuinely host-local), `i18n/en.json`, `i18n/zh-TW.json` (new keys for the 14 migrated messages: 6 `HostIo.ok` results, 3 "Node added", 2 kind-change/enum-commit, 1 Firefox-iOS save hint, 1 delete confirmation — name each `web.host.<action>` per existing touch key conventions)
- Test: `web/touch-render.spec.mjs`, manual touch pass

**Interfaces:**
- Consumes: `Intent::SetHostNotice` via the wasm FFI dispatch call (already exposed — same `dispatch(intent)` binding used for every other Intent)
- Produces: `toast()` becomes a pure severity-driven renderer (`toast(notice: Notice)`), never called with authored text again.

- [ ] **Step 1: Write the failing test for one representative case of each class**

```js
// web/touch-render.spec.mjs

test("clipboard-locked duplicate toast site is gone; core's own Warn renders instead", () => {
  // Pick one of the 17 pure-duplicate sites (e.g. selectOnly's guard at app.ts:531-533 per earlier audit).
  triggerLockedSelect();
  // Assert the notice bar (not a toast()) shows core.clipboard.action-locked text —
  // i.e. no toast() call fires, only the severity-driven status render from Task 12.
  assert.equal(lastToastCall(), null);
  assert.ok(document.querySelector("#status").textContent.includes(t("core.clipboard.action-locked")));
});

test("host-op guard (no core intent) still warns via SetHostNotice", () => {
  triggerOpenSaveSheetWhileLocked(); // one of the 7
  assert.ok(dispatchedIntents().some(i => i.type === "SetHostNotice" && i.key === "core.clipboard.action-locked"));
});

test("HostIo.ok result migrates with a catalog key", () => {
  triggerSaveSuccess();
  assert.ok(dispatchedIntents().some(i => i.type === "SetHostNotice" && i.key === "web.host.save-ok"));
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `node --test web/touch-render.spec.mjs`
Expected: FAIL — `toast()` sites still fire with authored text.

- [ ] **Step 3: Migrate each of the three classes**

1. **17 pure duplicates** (clipboard-locked guards on operations core also guards, e.g. `selectOnly`): delete the `toast(t("core.clipboard.action-locked"))` call entirely; the return-early guard logic stays (still needed to stop the local action), but the message now comes from core's own `guard_clipboard_locked` Warn via the normal `dispatch()` → `notice` → Task 12 rendering path — no client-side toast call needed.
2. **7 host-op guards** (save/open/lang sheets, format cycle, reorder, menu sheet — pure-UI paths core never sees): replace `toast(t("core.clipboard.action-locked"))` with `dispatch({ type: "SetHostNotice", key: "core.clipboard.action-locked", args: [], source: "host-web" })` — reuses the existing key, no new catalog entry needed.
3. **14 host-local messages**: replace each `toast("literal English")`/`toast(t("some.key"))` with `dispatch({ type: "SetHostNotice", key: "web.host.<new-key>", args: [...], source: "host-web" })`, adding the new key to `notice.rs`'s `severity_of` table (Task 1, requires a core-side change — coordinate: this Task 13 step depends on Task 1's file also being touched again here) and both catalogs.

Finally, change `toast()`'s own signature to take a rendered `Notice` and do nothing but the visual toast animation — delete every remaining call that passes raw text.

- [ ] **Step 4: Run test to verify it passes**

Run: `node --test web/touch-render.spec.mjs`
Expected: PASS.

- [ ] **Step 5: Manual touch pass**

Covering the 7 host-operation guards specifically (per spec §8 phase-3 verification) — open each of save/open/lang sheets, format cycle, reorder, and the menu sheet with clipboard armed, confirm a Warn still appears (now via `SetHostNotice` instead of a direct toast).

- [ ] **Step 6: Commit**

```bash
git add web/touch/app.ts crates/confy-core/src/session/notice.rs i18n/en.json i18n/zh-TW.json
git commit -m "refactor(touch): migrate 38 toast() call sites to severity-driven SetHostNotice (17 deleted / 7 host-notice / 14 migrated)"
```

---

## Task 14: Delete prompt fallback chain, `has_descendant_violation` rename, `schemaHintText` i18n

**Files:**
- Modify: `web/prompt.ts` (delete `PROMPT_QUESTIONS`, `promptQuestion()`, `web.prompt.confirmFallback` fallback and its catalog key), `web/panel.ts` (`schemaHintText` switches to `core.hint.enum`/`core.hint.bounded`), every file with `has_descendant_warning` (12-file blast radius per §11 Q11 — `grep -rn 'has_descendant_warning' web/ crates/`), `i18n/en.json`, `i18n/zh-TW.json` (new `core.hint.enum`, `core.hint.bounded`; delete `web.prompt.confirmFallback`)
- Test: `web/functional_smoke.mjs`, `web/render.spec.mjs`

**Interfaces:**
- Consumes: `ModeView.Prompt.question` (already wired in Task 12), `EditHint::describe` (Rust-side — confirm whether this needs a paired core change; if `EditHint::describe` already exists and just needs new keys, that's a small addition here, not a new task)
- Produces: `rows[].has_descendant_violation` replaces `rows[].has_descendant_warning` everywhere (Rust + TS).

- [ ] **Step 1: Write the failing test**

```js
test("prompt overlay uses mode.question directly, no PROMPT_QUESTIONS fallback", () => {
  const src = readFileSync("web/prompt.ts", "utf8");
  assert.ok(!src.includes("PROMPT_QUESTIONS"));
  assert.ok(!src.includes("promptQuestion"));
});

test("schemaHintText renders via core.hint.enum/bounded keys, not hardcoded English", () => {
  const src = readFileSync("web/panel.ts", "utf8");
  assert.ok(!src.includes('"Valid values:'));
  assert.ok(src.includes("core.hint.enum"));
});

test("has_descendant_violation replaces has_descendant_warning everywhere", () => {
  const hits = execSync("grep -rl has_descendant_warning web/ crates/ || true").toString();
  assert.equal(hits.trim(), "");
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `node --test web/functional_smoke.mjs`
Expected: FAIL on all three.

- [ ] **Step 3: Implement**

Delete `PROMPT_QUESTIONS`, `promptQuestion()`, and the `web.prompt.confirmFallback` catalog key + its usage; the overlay now reads `mode.question` directly (already the case if Task 12 wired it — this task is cleanup of the now-dead fallback code). Add `core.hint.enum: "Valid values: {0}"` / `core.hint.bounded: "Must be between {0} and {1}"` to both catalogs; update Rust `EditHint::describe(&self, lang: Lang)` to use them; update `panel.ts::schemaHintText` to call `tArgs` with the same keys. Rename `has_descendant_warning` → `has_descendant_violation` in every one of the 12 files (Rust struct field, TS interface, any render logic keying off the name).

- [ ] **Step 4: Run test to verify it passes**

Run: `node --test web/functional_smoke.mjs` and `cargo test -p confy-core`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add web/prompt.ts web/panel.ts crates/confy-core/src/ i18n/en.json i18n/zh-TW.json
git commit -m "refactor(web+core): delete prompt fallback chain, rename has_descendant_violation, i18n schema hints"
```

---

## Task 15: Delete `SessionSnapshot` legacy `status`/`error` fields (paired core+web)

**Files:**
- Modify: `crates/confy-core/src/session/view.rs` (remove `status`/`error` fields and the dual-write computation from Task 6), `web/types.ts` (remove `status`/`error` from the mirrored interface), every remaining web consumer of `snap.status`/`snap.error` (`grep -rn 'snap\.status\|snap\.error\|snapshot\.status\|snapshot\.error' web/`)
- Test: `web/functional_smoke.mjs` (full update, per §10 — the "trivial" partial update from Task 12 becomes complete here), `crates/confy-core/tests/session_headless.rs` (`error_text()`/`status_text()` now the ONLY way to read notice text — confirm no test still references `snap.status`/`snap.error` directly)

**Interfaces:**
- Consumes: `SessionSnapshot.notice` (already the primary field since Task 6)
- Produces: `SessionSnapshot` loses `status`/`error`. This is the breaking change §10 dual-write mitigation was protecting against — MUST land core+web in the same commit.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn session_snapshot_has_no_legacy_status_error_fields() {
    // Compile-time check via a match — if `status`/`error` still exist this
    // won't compile once the struct literal below omits them and the type
    // has #[non_exhaustive] removed... simplest is a doc-comment-driven grep test:
    let src = std::fs::read_to_string("src/session/view.rs").unwrap();
    assert!(!src.contains("pub status: Option<String>"));
    assert!(!src.contains("pub error: Option<String>"));
}
```
```js
test("web consumers use snap.notice, not snap.status/snap.error", () => {
  const files = execSync("grep -rl 'snap\\.status\\|snap\\.error' web/ || true").toString().trim();
  assert.equal(files, "");
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p confy-core session_snapshot_has_no_legacy` and `node --test web/functional_smoke.mjs`
Expected: FAIL — fields still present.

- [ ] **Step 3: Implement**

Remove `status`/`error` fields and their dual-write computation from `view.rs`. Update every remaining web reader to use `snap.notice` directly (severity + text), matching the rendering already implemented in Task 12. Update `functional_smoke.mjs`'s remaining legacy-field assertions (the ones Task 12 left as "trivial, full update in phase 3" per §10) to assert against `notice`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p confy-core`, `node --test web/functional_smoke.mjs`, `node --test web/render.spec.mjs web/touch-render.spec.mjs`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/confy-core/src/session/view.rs web/
git commit -m "refactor(core+web): remove legacy SessionSnapshot status/error fields, web reads notice exclusively"
```

**Phase 3 checkpoint:** `functional_smoke.mjs` (92 checks), `render.spec.mjs`, `touch-render.spec.mjs`, `vscode-schema-url.spec.mjs` green; manual touch pass covering the 7 host-operation guards done in Task 13.

---

## Task 16: CLI i18n

**Files:**
- Modify: the CLI's `convert` subcommand source (locate via `grep -rln 'fn main\|clap::Parser\|convert' crates/confy-tui/src crates/*/src --include=*.rs | grep -i cli`; spec calls it `cli.rs` — confirm exact path before starting), `i18n/en.json`, `i18n/zh-TW.json` (~10 new `cli.*` keys: convert warning list title/note, proceed question, create-file question, download-save question, wrote confirmation, unknown `--lang` warning)
- Test: `crates/confy-tui/tests/convert_cli.rs` (or wherever CLI integration tests live — same discovery grep)

**Interfaces:**
- Consumes: `tr`, `tr_args`, `Lang::from_str` (existing `i18n.rs`)
- Produces: no new public API; CLI lang resolution order becomes `--lang` flag > config file > `en` (today the `convert` subcommand path does not load the config file for lang — this task adds that).

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn convert_cli_respects_config_file_lang_when_no_flag() {
    // Write a temp config with lang: zh-TW, run `convert` without --lang,
    // assert the "wrote" confirmation string is the zh-TW rendering.
    let output = run_convert_cli(&["input.toml", "output.json"], /* cwd with zh-TW config */);
    assert!(output.contains(&tr(Lang::ZhTw, "cli.wrote")));
}

#[test]
fn convert_cli_unknown_lang_flag_warns_in_catalog_text() {
    let output = run_convert_cli(&["--lang", "xx", "input.toml", "output.json"], /* default cwd */);
    assert!(output.contains(&tr(Lang::En, "cli.unknown-lang")));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p confy-tui --test convert_cli`
Expected: FAIL — CLI hardcodes English, doesn't load config for lang.

- [ ] **Step 3: Implement**

Add the ~10 `cli.*` keys to both catalogs (exact final key names decided at implementation time per spec §6 — "exact set finalized in the plan" means: finalize here, not defer further). Thread `Lang` resolution (`--lang` flag > config file's lang field > `Lang::En`) into the CLI entry point; replace every hardcoded English string in the CLI path with `tr`/`tr_args` calls.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p confy-tui --test convert_cli`
Expected: PASS.

- [ ] **Step 5: Extend the catalog key-set equality test**

Find the existing en/zh-TW mirror test (`grep -rn 'key-set\|catalog.*equal\|en\.json.*zh' crates/confy-core/src/session/i18n.rs`); confirm it already covers the new `cli.*` keys automatically (it should, if it's a blanket key-set comparison) — if it's scoped to `core.*`/`tui.*` only, widen it.

Run: `cargo test -p confy-core i18n::`
Expected: PASS, catalogs still 1:1.

- [ ] **Step 6: Commit**

```bash
git add crates/confy-tui/src/ i18n/en.json i18n/zh-TW.json
git commit -m "feat(cli): route all convert-subcommand strings through i18n catalog, load config for --lang resolution"
```

**Phase 4 checkpoint:** `cargo test -p confy-tui --test convert_cli` green; catalog key-set equality test green.

---

## Task 17: Diag exports — FFI `diag_log()` + web `?diag=1` console drain

**Files:**
- Modify: the wasm FFI crate (locate exact path via `grep -rln 'wasm_bindgen' crates/ --include=*.rs | grep -v confy-core/src/session` — likely `crates/confy-ffi/src/lib.rs` or similar; confirm before starting), `web/ui.ts` (`?diag=1` drain logic)
- Test: `web/functional_smoke.mjs` extension (per spec §8 phase 5)

**Interfaces:**
- Consumes: `Session.diag: DiagRing` (Task 3), `#[wasm_bindgen]` export pattern already used elsewhere in the FFI crate for other session queries
- Produces: `diag_log() -> JsValue` (serialized `Vec<DiagEvent>`, oldest first); web-side `diagLog(): DiagEvent[]`.

- [ ] **Step 1: Write the failing test**

```js
test("?diag=1 logs only new events per keypress, diffed by seq", async () => {
  setUrlParam("diag", "1");
  const before = consoleDebugCalls().length;
  dispatchKeypress("j"); // any nav key
  const after1 = consoleDebugCalls().length;
  assert.ok(after1 > before);
  dispatchKeypress("j");
  const after2 = consoleDebugCalls().length;
  // second keypress logs only its own new events, not a re-print of the first's
  assert.equal(after2 - after1, newEventsSinceLastDrain());
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `node --test web/functional_smoke.mjs`
Expected: FAIL — `diag_log()`/drain don't exist.

- [ ] **Step 3: Implement**

FFI crate — add alongside existing `#[wasm_bindgen]` query exports:
```rust
#[wasm_bindgen]
pub fn diag_log(&self) -> JsValue {
    serde_wasm_bindgen::to_value(&self.session.diag.iter().collect::<Vec<_>>()).unwrap()
}
```
(Requires `DiagEvent` to derive `Serialize` — check Task 2's derive list; if it wasn't added there because diag was meant to stay Rust-internal, add `serde::Serialize` to `DiagEvent`'s derive list now, as a one-line addition, not a redesign.)

`web/ui.ts`:
```ts
let lastSeenSeq = -1;
function drainDiagIfEnabled() {
  if (new URLSearchParams(location.search).get("diag") !== "1") return;
  const events = session.diagLog() as DiagEvent[];
  for (const e of events) {
    if (e.seq <= lastSeenSeq) continue;
    console.debug(`[confy-diag] [${e.level}] ${e.kind} ${e.detail}`);
    lastSeenSeq = e.seq;
  }
}
```
Call `drainDiagIfEnabled()` after every `dispatch()` in the main render loop.

- [ ] **Step 4: Run test to verify it passes**

Run: `node --test web/functional_smoke.mjs`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/confy-ffi/src/ web/ui.ts
git commit -m "feat(ffi+web): export diag_log(), web ?diag=1 drains ring diffed by seq"
```

**Phase 5 checkpoint:** `functional_smoke.mjs` extension green.

---

## Task 18: Docs pass

**Files:**
- Modify: `docs/TUI.md` (diag overlay `~`, severity rendering), `docs/WEBUI.md` (toast unification, `?diag=1`), `CONTEXT.md` (confirm Notice/Severity/Prompt question/Diagnostic event entries match shipped behavior — they were pre-written during the spec grill; verify no drift), `CLAUDE.md` (module map: `notice.rs`, `diag.rs`, `overlay_diag.rs`), `CHANGELOG.md` (final `Unreleased Update` entry for the whole feature, or confirm the six phase commits' individual entries suffice — pick one approach and be consistent with this repo's existing changelog granularity, which is per-commit `Unreleased Update` entries, not one giant entry)
- Test: none (docs-only) — verification is a consistency pass, not a command

- [ ] **Step 1: Consistency pass**

Read each modified doc file against the shipped code (not the spec — the spec is the plan, the code is the truth) and fix any drift: confirm `~` binding is documented in `docs/TUI.md`'s keybinding table, confirm `docs/WEBUI.md` documents the unified toast behavior and `?diag=1`, confirm `CONTEXT.md`'s existing Notice/Severity/Prompt question/Diagnostic event entries (written during the spec grill, Tasks in this plan didn't change the model from what was specced) still match, confirm `CLAUDE.md`'s module map lists `notice.rs`/`diag.rs`/`overlay_diag.rs`.

- [ ] **Step 2: Add final CHANGELOG entry**

Following this repo's existing pattern (see the two spec-review entries already in `CHANGELOG.md`'s `[Unreleased]` section) — one `Unreleased Update` entry per phase commit is consistent with existing granularity; if all 18 tasks landed as fewer, squashed commits instead, write one entry per actual commit.

- [ ] **Step 3: Commit**

```bash
git add docs/TUI.md docs/WEBUI.md CONTEXT.md CLAUDE.md CHANGELOG.md
git commit -m "docs: message-system integration — TUI.md/WEBUI.md/CONTEXT.md/CLAUDE.md/CHANGELOG updates"
```

**Phase 6 checkpoint:** all docs consistent with shipped behavior. Full feature done — all six spec §8 phases green.

---

## Self-Review Notes (for the plan author, kept for the executor's context)

- **Spec coverage:** §1 motivation → Tasks 1-17 collectively. §2/2.1/2.2 → Tasks 1,4,5. §3 wire contract → Tasks 5,6,12,15. §4 prompt consolidation → Tasks 5,8,14. §5.1 TUI → Tasks 7,8,9. §5.2 web → Tasks 12,13,14. §5.3 shared strings → Tasks 10,14. §6 CLI → Task 16. §7 diagnostics → Tasks 2,3,6,11,17. §8 phasing → task groups map 1:1 to phase rows. §9 non-goals → respected (no queue UI, no tracing dep, no schema payload restructuring, no VS Code new channels, no touch sheet inventory change — none of Tasks 1-18 touch these). §10 dual-write → Tasks 6 (add) and 15 (remove, paired). §11/§12 review decisions → folded into Global Constraints and individual task rationale comments throughout.
- **Known discovery gaps left for the executor** (paths not fully confirmed by the grill's scouts, flagged explicitly rather than guessed): the exact CLI source file (Task 16), the exact wasm FFI crate path (Task 17), whether `EditHint::describe` needs a paired signature change (Task 14) — each task's Step 1 or implementation step includes the `grep`/discovery command to resolve it before writing code, rather than the plan guessing wrong.
- **Type consistency check:** `Notice`/`Severity`/`NoticeSource` signatures (Task 1) are reused identically in Tasks 3,6,7,8,9,12,13,17 without renaming. `DiagEvent`/`DiagLevel`/`DiagRing` (Task 2) reused identically in Tasks 3,11,17. `Intent::SetHostNotice{key,args,source}` (Task 6) reused identically in Tasks 9,13. `error_text()`/`status_text()` (Task 6) reused in Task 15's cleanup. No signature drift found.
</content>
