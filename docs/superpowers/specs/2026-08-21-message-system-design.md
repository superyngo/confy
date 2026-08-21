# Message System Integration — Design

**Date:** 2026-08-21
**Status:** Approved in chat; pending spec review
**Scope:** confy-core message model, SessionSnapshot wire contract, all hosts (TUI / Web desktop / Touch / VS Code), CLI i18n, new diagnostics layer

---

## 1. Motivation

The 2026-08-21 inventory (four-scout audit) found the user-facing message system
has exactly two severity buckets (`Session.status` / `Session.error`, plain
strings) with inconsistent boundaries, prompt question text multiplexed onto
`status`, two parallel prompt-text families (`core.*` status-carried vs
`tui.prompt.*` overlay-carried, plus a web `PROMPT_QUESTIONS` fallback for the
one prompt core never gave text to), host errors written raw into core fields
with no provenance, English-only hint text bypassing i18n in two places, a
transient toast channel that exists only on touch, three hand-rolled
"N schema warnings" aggregations, CLI strings outside the catalog, and no
diagnostic layer at all (zero `log`/`tracing` usage).

User decisions (2026-08-21):
- **(a)** Adopt a typed severity taxonomy carried on the wire.
- **(b)** Add a developer-facing diagnostics layer.
- **(c)** Delegated: prompt question text moves out of `status` into
  `ModeView::Prompt` (decided: yes).
- Desktop web's missing toast is a bug, not a design choice — unify.
- TUI host errors writing `session.error` directly is bug-grade debt — fix with
  provenance.
- CLI strings join the i18n catalog.

## 2. Core model (confy-core)

New file `crates/confy-core/src/session/notice.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity { Info, Success, Warn, Error }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NoticeSource { Core, HostTui, HostWeb }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notice {
    pub severity: Severity,
    pub text: String,          // i18n-rendered, ready to display
    pub source: NoticeSource,  // default Core
}
```

- `Session.status: Option<String>` and `Session.error: Option<String>` are
  replaced by a single `Session.notice: Option<Notice>`.
- Lifecycle is unchanged: single slot, replaced by the next notice, cleared on
  mutation success / Esc / edit begin / language switch (SetLang intent clears
  stale rendered text), exactly where `status`/`error` are cleared today.
- Helpers on Session: `fn notice_info/text…` are NOT added; call sites use
  `self.notice = Some(Notice::core(Severity::Warn, tr_args(...)))` style
  constructors (`Notice::core`, `Notice::host_tui`) to keep source consistent.

### 2.1 Severity classification rules

| Severity | Meaning | Rule |
|---|---|---|
| `Error` | An operation the user initiated **failed** | mutation apply error, host I/O failure, schema load failure |
| `Warn` | Action unavailable in current context; user stays in flow | readonly / locked / unsupported / invalid-input / precondition-unmet guidance |
| `Success` | Action completed | mutation confirmations |
| `Info` | Neutral state report | empty/nothing/cancelled/aborted notices |
| *(question)* | Prompt awaiting an answer | moved out of Notice into `ModeView::Prompt` (§3) |

### 2.2 Per-key mapping table (all non-`detail` `core.*` keys, 45 total)

**Error (11):** `core.error.generic`, `core.add.error`, `core.delete.error`,
`core.paste.error`, `core.paste.comment-illegal`, `core.remark.error`,
`core.rename.failed`, `core.trailing.update-failed`, `core.undo.error`,
`core.redo.error`, `core.kind-switch.error`.

**Warn (14):** `core.readonly`, `core.clipboard.action-locked`,
`core.comment.unsupported`, `core.trailing.inline-unsupported`,
`core.reveal.hidden-by-filter`, `core.move.self`, `core.insert.collision`,
`core.rename.empty-key`, `core.value.invalid`, `core.comment.invalid`,
`core.fragment.invalid`, `core.remark.invalid`, `core.convert.root-only`,
`core.kind-switch.unsupported`.

**Success (7):** `core.save.saved`, `core.kind-switch.converted`,
`core.kind-switch.converted-generic`, `core.clipboard.cut`,
`core.clipboard.copied`, `core.clipboard.cut-changed`,
`core.clipboard.copied-changed`.

**Info (9):** `core.save.nothing`, `core.clipboard.empty`,
`core.clipboard.cleared`, `core.selection.cleared`, `core.undo.empty`,
`core.redo.empty`, `core.paste.cancelled`, `core.add.placeholder`,
`core.convert.aborted`.

**Question (moved):** `core.paste.collision`, `core.quit.confirm`,
`core.type-change`, `core.paste.array-upgrade-confirm` become
`core.prompt.collision`, `core.prompt.confirm-quit`, `core.prompt.type-change`,
`core.prompt.array-upgrade`, plus new `core.prompt.jsonc-upgrade` — all with
embedded key legends (`? y/n`, `— o/r/c`) REMOVED from the string.

When a prompt opens, the site sets only the mode (with question text); the old
pattern of simultaneously writing `error`/`status` (e.g. clipboard.rs:263-264)
is deleted — the question is the message.

## 3. Wire contract (`SessionSnapshot`)

```
- status: Option<String>          → notice: Option<Notice>
- error: Option<String>           → (removed)
- ModeView::Prompt { kind }       → ModeView::Prompt { kind, question: String }
- rows[].has_descendant_warning   → rows[].has_descendant_violation
```

- `question` is rendered **per snapshot** by `prompt_view()` from `PromptKind`
  data (i18n from `Session.lang`), deterministic since snapshots rebuild the
  view. Runtime language switches (SetLang) re-render the prompt correctly.
  Hosts never reconstruct it.
- `web/types.ts` mirrors all changes. VS Code uses the same web bundle — no
  extra interface. wasm + web ship together in this repo, so these are
  one-shot breaking changes inside paired phases (core + web land before the
  next release; see §10 dual-write mitigation).
- `Notice` serializes as `{severity, text, source}`; severity/source as above.

## 4. Prompt text consolidation

Today: `core.*` texts carried on `status` (web reads `snap.status ??
snap.error`, strips trailing legend), `tui.prompt.*` texts with embedded
legends (TUI overlay), web `PROMPT_QUESTIONS` fallback covering the
`JsoncUpgrade` gap (core never set a status for it).

After:
- One question string per prompt: `core.prompt.<kind>` (5 keys), legend-free.
- TUI overlay (`draw_prompt_overlay`) keeps its 3-line dialog and appends a
  legend line rendered from localized `tui.prompt.<kind>.legend` keys
  (renamed from today's `tui.prompt.<kind>`; text keeps only the legend part,
  e.g. `o:overwrite  r:rename  c:cancel`).
- Web `#overlay` / touch `.prompt-sheet` use `mode.question` directly;
  `PROMPT_QUESTIONS` fallback and the `promptQuestion()` strip-legend hack are
  deleted. `web.prompt.title.*` + `web.prompt.btn.*` stay.

## 5. Host presentation mapping

### 5.1 TUI

| Severity | Rendering |
|---|---|
| Success | status line green (current success color) |
| Info | status line default color |
| Warn | status line yellow |
| Error | status line red-bg white-text (current error style) |

`draw_status` priority: **Error notice** (preserves existing "errors never
hidden" invariant, `ui.rs:481-482`) > **active input** (Filter query / Edit
mode) > **Warn/Success/Info notice** > edit-mode hint. The "N schema warnings"
footer switches to the shared key (§5.3). TUI host error sites (`app.rs`
save/editor/convert-write, `schema_io.rs` fetch, config write) call
`set_host_notice` with `NoticeSource::HostTui` + `Severity::Error`.

### 5.2 Web desktop + VS Code webview

| Severity | Rendering |
|---|---|
| Success | **new toast** (1.6 s auto-hide, same animation as touch) + status bar text |
| Info | status bar |
| Warn | status bar, `.sev-warn` tint |
| Error | status bar red (`.err` today → `.sev-error`), click-to-clear kept |

- One toast element (`#toast`) in `index.html`; logic mirrors
  `touch/app.ts::toast()`; no queue — a new toast replaces the showing one.
- Touch keeps its current behavior (success→toast, else statusbar), now driven
  by `notice.severity` instead of per-call-site choice; the 20+ hardcoded
  `toast(...)` call sites that duplicate core messages are replaced by the
  severity-driven path.
- **Web host messages** (e.g. `recentGone` recent-file vanished, schema fetch
  failures) migrate to FFI `set_host_notice(severity: Severity, text: String)`
  (stamped with `NoticeSource::HostWeb`), so they enter the single-slot model
  and appear in diagnostics. Touch-only local notices (Firefox iOS save hint,
  if still purely host-side) may call the same FFI or remain DOM-local.
- VS Code extension-side `showErrorMessage` / `showInformationMessage` stay
  native; their severity mapping is already error/info.
### 5.3 Shared strings

- New `core.schema.count` (`"{0} schema warning(s)"` / zh-TW) — TUI footer,
  web status append, and touch fallback all format
  `schema_status.violation_count` through it; the three hand-rolled strings
  are deleted.
- `EditHint::describe(&self, lang: Lang)` renders via new keys
  `core.hint.enum` (`"Valid values: {0}"`) and `core.hint.bounded`
  (`"Must be between {0} and {1}"`); web `panel.ts::schemaHintText` switches
  to the same keys via `tArgs` — the English-only bypasses are removed.

## 6. CLI i18n

- New `cli.*` namespace keys (~10): convert warning list title/note, proceed
  question, create-file question, download-save question, wrote confirmation,
  unknown `--lang` warning. Exact set finalized in the plan.
- `cli.rs` renders all user-facing strings via `tr`/`tr_args`; lang resolution
  `--lang` > config file > `en`, and the `convert` subcommand path loads the
  config file for lang (today it does not).
- `en.json` + `zh-TW.json` updated in lockstep; the existing mirror test (if
  any) is extended to assert key-set equality across namespaces.

## 7. Diagnostics layer (new, developer-facing, English, no i18n)

New file `crates/confy-core/src/session/diag.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagLevel { Debug, Info, Warn, Error }

#[derive(Debug, Clone)]
pub struct DiagEvent {
    pub seq: u64,                // monotonic per Session
    pub level: DiagLevel,
    pub kind: &'static str,      // "dispatch" | "mutation" | "schema" | "convert" | "host_notice"
    pub detail: String,          // English, structured-ish "key=value" fragments
}
```

- `Session.diag: VecDeque<DiagEvent>`, ring capacity **256** (oldest evicted).
- Recorded events (initial set, keep minimal):
  `dispatch` (intent name, in Debug), `mutation` (variant, ok/err + error
  variant, Info/Error), `schema` (detect source, validate violation count,
  load failure, Info/Error), `convert` (target format, warnings count, abort,
  Info/Warn), `host_notice` (severity, source, Info).
- Zero new dependencies — no `tracing` (global state fights the pure, fully
  unit-testable Session; wasm size). `no_fs_gate.rs` stays green.
- Exports:
  - TUI: `~` opens a read-only diag overlay (`overlay_diag.rs`, reuses the
    Help popup shape; newest at bottom; `~`/Esc closes).
  - FFI: `diag_log() -> JsValue` (serialized Vec, oldest first).
  - Web: when `?diag=1` is present, `ui.ts` drains `diagLog()` after each
    dispatch into `console.debug` with a `[confy-diag]` prefix.

## 8. Phasing (each phase compiles + tests green, one commit each)

| # | Phase | Verification |
|---|---|---|
| 1 | Core: `notice.rs` model, snapshot fields, per-site re-tier, prompt question field, prompt key consolidation, catalog updates (en+zh-TW) | `cargo test -p confy-core`; new test asserting every core.* non-detail key maps to exactly one severity (table-driven) |
| 2 | TUI: severity rendering, legend keys, `set_host_notice` migration, `core.schema.count` footer, `~` diag overlay + diag recording | `cargo test -p confy-tui`; manual TUI pass |
| 3 | Web: `types.ts` mirror, `notice`/`question` consumption, desktop `#toast`, severity classes, touch severity-driven toast, `schemaHintText` i18n, delete strip hack + fallback array | `functional_smoke.mjs` (92 checks, updated), `render.spec.mjs`, `touch-render.spec.mjs`, `vscode-schema-url.spec.mjs` |
| 4 | CLI: `cli.*` keys + `tr()` everywhere + convert-path config load | `cargo test -p confy-tui --test convert_cli`; catalog key-set equality test |
| 5 | Diag exports: FFI `diag_log()`, web `?diag=1` console drain | `functional_smoke.mjs` extension |
| 6 | Docs: TUI.md, WEBUI.md, CONTEXT.md (glossary: Notice/Severity), CLAUDE.md module map, CHANGELOG | consistency pass |

## 9. Non-goals

- No message history / queue UI for user notices (single slot stays).
- No `tracing`/`log` dependency, no file logging, no log-level config plumbing.
- No restructuring of schema Violation or ConvertWarning payloads (they keep
  their own channels; only their aggregate/hint strings share keys).
- No VS Code-specific new channels beyond existing `show*Message`.
- No change to touch sheet inventory or `web.prompt.btn.*` structure.

## 10. Risks / notes

- `SessionSnapshot` break: **dual-write mitigation** — Phase 1 populates BOTH
  `notice` (new) and legacy `status`/`error` fields (computed from notice) so
  old web JS keeps working. Phase 3 switches web consumers to `notice` and
  **removes** the legacy fields in the same commit (core+web together). Every
  intermediate commit stays green. Phase 1 also updates `functional_smoke.mjs`
  expectations minimally (legacy field assertions → notice assertions where
  trivial; full update in phase 3).
- zh-TW translations for new keys: author in phase 1 alongside en (mirror
  test enforces).
