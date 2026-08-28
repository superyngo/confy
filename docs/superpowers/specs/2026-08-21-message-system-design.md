# Message System Integration — Design

✅ **Shipped — historical reference.** See `CHANGELOG.md` for current behavior; this design record is kept for context, not as a live specification.

**Date:** 2026-08-21
**Status:** Approved (chat 2026-08-21); first spec review 2026-08-21 — decisions in §11; second review 2026-08-21 — decisions in §12
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
- Lifecycle is nearly unchanged: single slot, replaced by the next notice,
  cleared on mutation success / Esc / edit begin exactly where `status`/`error`
  are cleared today. One **new** clear: SetLang (today `set_lang` clears
  nothing — stale old-language text lingers; §11 Q12).
- `NoticeSource` is a closed set: Tauri/touch/VS Code all ride the web bundle →
  `HostWeb`; a variant is added only when a real fourth host exists (paired
  wasm+web shipping makes that break cheap).
- Helpers on Session: `fn notice_info/text…` are NOT added. Severity is **not**
  spelled at call sites: one `severity_of(key) -> Severity` table in
  `notice.rs` is the single source of truth, and sites read
  `self.notice = Some(Notice::core(key, args))` — the constructor looks the
  severity up. Mis-tiering a site becomes impossible rather than merely
  tested (§12 Q1). Host notices take the same key+args shape through
  `Intent::SetHostNotice` (§12 Q5, Q6): they get catalog keys as they
  migrate (§11 Q10, §12 Q8) and resolve severity through the same
  `severity_of` table — **no explicit-severity variant exists**, so a host
  message with no catalog key is not representable and must get one before
  it migrates. `Notice::host_tui` / `Notice::host_web` are internal to that
  handler, never a host-facing API.

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
- The `has_descendant_violation` rename lands in **Phase 3's** paired commit —
  not Phase 1 (§11 Q11).

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
  `PROMPT_QUESTIONS`, the `promptQuestion()` strip-legend hack, **and** its
  final `web.prompt.confirmFallback` fallback (a third fallback the original
  list missed — `prompt.ts:63`) are deleted. `web.prompt.title.*` +
  `web.prompt.btn.*` stay.

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
mode) > **Warn/Success/Info notice** > edit-mode hint. The non-Error demotion
below active input is **rendering-only** — the Notice stays in the slot and
reappears when input exits (§11 Q4). The "N schema warnings"
footer switches to the shared key (§5.3). TUI host error sites (`app.rs`
save/editor/convert-write, `schema_io.rs` fetch, config write) stop writing
`session.error` through the public field and instead dispatch
`Intent::SetHostNotice` with `NoticeSource::HostTui` — keeping `dispatch` the
sole mutation path (§12 Q6).

### 5.2 Web desktop + VS Code webview

| Severity | Rendering |
|---|---|
| Success | **new toast** (1.6 s auto-hide, same animation as touch) + status bar text |
| Info | status bar |
| Warn | status bar, `.sev-warn` tint |
| Error | status bar red (`.err` today → `.sev-error`), click-to-clear kept |

- One toast element (`#toast`) in `index.html`; logic mirrors
  `touch/app.ts::toast()`; no queue — a new toast replaces the showing one.
- **One severity→surface rule for every web host** (touch included):
  Success ⇒ toast **and** status-bar text; Info/Warn ⇒ status bar;
  Error ⇒ red bar + click-to-clear — all driven by `notice.severity` (touch
  gains the status-bar text; §11 Q5). The **38** `toast(...)` call sites in
  `touch/app.ts` break down as: **17** clipboard-locked duplications of core's
  `guard_clipboard_locked` message (deleted outright), **7** clipboard-locked
  guards on *host* operations that dispatch no intent — save/open/lang sheets,
  format cycle, reorder, menu sheet — which become host notices reusing
  `core.clipboard.action-locked` (§12 Q8), and **14** genuinely host-local
  messages (6 `HostIo.ok` results, 3 "Node added", 2 kind-change/enum-commit,
  1 Firefox-iOS save hint, 1 delete) which migrate the same way, picking up
  catalog keys as they move (§11 Q10). `toast()` survives only as a *render*
  function of severity, never called with authored text. The 7 host-operation
  guards are the reason this is not a flat delete: core never sees those
  actions, so deleting their toast would ship silence (§12 Q8).
- **Web host messages** (e.g. `recentGone` recent-file vanished, schema fetch
  failures) migrate to `Intent::SetHostNotice { key, args, source }`
  (stamped `NoticeSource::HostWeb`), not a bespoke FFI setter: `dispatch` is
  the only mutation path today and stays that way (§12 Q6), and passing
  key+args rather than rendered text keeps `Session.lang` the single language
  authority — `web/i18n.ts` reads the same repo-root catalog, but it renders
  with its own `getLang()`, which can drift (§12 Q5). They enter the
  single-slot model and appear in diagnostics. Host-origin Errors follow the
  uniform single-slot lifecycle — no stickiness; the next Info/Success
  displaces them, and the diag ring preserves the history (§11 Q9).
  Touch-only local notices (Firefox-iOS save hint) travel the same Intent
  (§11 Q10, §12 Q8).
- VS Code extension-side `showErrorMessage` / `showInformationMessage` stay
  native; their severity mapping is already error/info. This is a deliberate,
  permanent carve-out with known costs — recorded in §9 (§12 Q4).

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
    pub kind: &'static str,      // "dispatch" | "mutation" | "schema" | "convert" | "notice"
    pub detail: String,          // English, structured-ish "key=value" fragments
}
```

- `Session.diag: VecDeque<DiagEvent>`, ring capacity **256** (oldest evicted).
- Recorded events (initial set, keep minimal):
  `dispatch` (intent name, in Debug), `mutation` (variant, ok/err + error
  variant, Info/Error), `schema` (detect source, validate violation count,
  load failure, Info/Error), `convert` (target format, warnings count, abort,
  Info/Warn), `notice` (**every** notice assignment — severity, source,
  catalog key, and rendered text captured **verbatim** — recorded by one tap
  inside the setter, so "what did the user see, in order" is answerable and
  core/host notices are not asymmetric; §12 Q3). All `dispatch` events are
  recorded unfiltered — Debug
  marks the noise; navigation churning the ring is accepted (§11 Q2). Captured
  payloads may be non-English: the English-only rule governs *authored*
  fragments, not captured text (§11 Q3).
- Zero new dependencies — no `tracing` (global state fights the pure, fully
  unit-testable Session; wasm size). `no_fs_gate.rs` stays green.
- Exports:
  - TUI: `~` opens a read-only diag overlay (`overlay_diag.rs`, reuses the
    Help popup shape; newest at bottom; `~`/Esc closes).
  - FFI: `diag_log() -> JsValue` (serialized Vec, oldest first).
  - Web: when `?diag=1` is present, `ui.ts` drains `diagLog()` after each
    dispatch into `console.debug` with a `[confy-diag]` prefix, **diffing by
    `seq`** (a module-level last-seen counter) so each keypress logs only new
    events — never a full re-print (§11 Q14).

## 8. Phasing (each phase compiles + tests green, one commit each)

| # | Phase | Verification |
|---|---|---|
| 1 | Core: `notice.rs` model + `severity_of` table, snapshot fields, per-site re-tier, prompt question field, prompt key consolidation, catalog updates (en+zh-TW) | `cargo test -p confy-core`; new test asserting every core.* non-detail key resolves through `severity_of` to exactly one severity; a new test asserting slot occupancy — a `Warn` populates `notice`/`error_text() == None`/`status_text() == Some`, distinct from the `None → both None` and `Error → error_text` cases — so single-slot behavior is actually exercised, not just the old two-bucket shape (§12 Q7 follow-up); the 82 existing Some/None assertions migrate via test-only `snap.error_text()` / `snap.status_text()` helpers preserving their original meaning (§12 Q7) |
| 2 | TUI: severity rendering, legend keys, `Intent::SetHostNotice` migration of direct field writes, `core.schema.count` footer, `~` diag overlay + diag recording | `cargo test -p confy-tui`; manual TUI pass |
| 3 | Web: `types.ts` mirror, `notice`/`question` consumption, desktop `#toast`, severity classes, touch severity-driven toast (17 deleted / 7 host-notice / 14 migrated, §12 Q8), `schemaHintText` i18n, `has_descendant_violation` rename (paired core+web), delete strip hack + fallback array + `confirmFallback` | `functional_smoke.mjs` (92 checks, updated), `render.spec.mjs`, `touch-render.spec.mjs`, `vscode-schema-url.spec.mjs`; manual touch pass covering the 7 host-operation guards |
| 4 | CLI: `cli.*` keys + `tr()` everywhere + convert-path config load | `cargo test -p confy-tui --test convert_cli`; catalog key-set equality test |
| 5 | Diag exports: FFI `diag_log()`, web `?diag=1` console drain | `functional_smoke.mjs` extension |
| 6 | Docs: TUI.md, WEBUI.md, CONTEXT.md (glossary: Notice/Severity), CLAUDE.md module map, CHANGELOG | consistency pass |

## 9. Non-goals

- No message history / queue UI for user notices (single slot stays).
- No `tracing`/`log` dependency, no file logging, no log-level config plumbing.
- No restructuring of schema Violation or ConvertWarning payloads (they keep
  their own channels; only their aggregate/hint strings share keys).
- No VS Code-specific new channels beyond existing `show*Message`. The
  extension host is a separate process from the webview, so its messages stay
  outside i18n, outside the Notice slot, and outside the diag ring — a known
  blind spot when triaging "saving did nothing" reports (§12 Q4).
- No change to touch sheet inventory or `web.prompt.btn.*` structure.

## 10. Risks / notes

- `SessionSnapshot` break: **dual-write mitigation** — Phase 1 populates BOTH
  `notice` (new) and legacy `status`/`error` fields (computed from notice;
  mapping pinned: `Error → error`, `Info/Success/Warn → status`,
  `None → both None` — old web JS renders identically pre/post Phase 1; §11
  Q6) so
  old web JS keeps working. Phase 3 switches web consumers to `notice` and
  **removes** the legacy fields in the same commit (core+web together). Every
  intermediate commit stays green. Phase 1 also updates `functional_smoke.mjs`
  expectations minimally (legacy field assertions → notice assertions where
  trivial; full update in phase 3).
- zh-TW translations for new keys: author in phase 1 alongside en (mirror
  test enforces).

## 11. Spec review decisions (2026-08-21 grill)

Fifteen questions across three rounds, all settled; recommendations accepted
as proposed. Amendments above reference these by number.

**Design decisions:**

- **Q1** `NoticeSource` stays a closed 3-value enum; Tauri/touch/VS Code are
  `HostWeb`; a new variant only when a real fourth host exists.
- **Q2** Diag ring records **all** `dispatch` events unfiltered (Debug marks
  the noise); navigation churning the 256-ring is accepted.
- **Q3** `host_notice` diag events capture the notice's rendered text
  verbatim — the English-only rule governs authored fragments, not captured
  payloads (and the text is exactly what i18n debugging needs). *(Kind
  renamed `notice` and widened to every notice — superseded by §12 Q3.)*
- **Q4** TUI non-Error notices demoted below active input is rendering-only
  (the Notice stays in the slot and reappears on input exit); Error keeps the
  never-hidden invariant.
- **Q5** One severity→surface rule for every web host: Success ⇒ toast +
  status-bar text; Info/Warn ⇒ status bar; Error ⇒ red bar + click-to-clear.
- **Q6** Dual-write legacy mapping pinned: `Error → error`,
  `Info/Success/Warn → status`, `None → both None`.
- **Q7** User-facing copy keeps "warning(s)" (`core.schema.count`); the
  glossary avoid-rule targets type names, which the
  `has_descendant_violation` rename already honors.
- **Q8** Glossary updates (landed during review): `CONTEXT.md` **Notice**
  entry gained its NoticeSource sentence; new **Prompt question** entry.
- **Q9** Host-origin Errors follow the uniform single-slot lifecycle — no
  stickiness; the diag ring preserves history.
- **Q10** All user-facing host-local messages (incl. touch's Firefox-iOS
  hint) route through FFI `set_host_notice` *(channel superseded by
  `Intent::SetHostNotice` — §12 Q6)*; `toast()` becomes purely a
  severity renderer; hardcoded-English strings get catalog keys as they
  migrate.
- **Q11** `has_descendant_warning → has_descendant_violation` rename moved to
  Phase 3's paired commit (12-file blast radius, cosmetic, unasserted by any
  test today).
- **Q12** SetLang clears the notice — **new** behavior (today it clears
  nothing); re-render-from-key was rejected (contradicts the rendered-only
  `Notice` model).
- **Q13** `web.prompt.confirmFallback` is deleted with the rest of the
  fallback chain; an empty question is the honest failure signal.
- **Q14** Web `?diag=1` drain diffs by `seq` (module-level last-seen counter)
  — never a full re-print per keypress.
- **Q15** This section records the review; the header status is flipped.

**Fact verifications** (sub-agent audits, 2026-08-21):

- Catalog: exactly 45 non-detail `core.*` keys in `i18n/en.json`, 1:1 with
  §2.2's table — no gaps in either direction.
- `set_lang` (`session.rs:111`) assigns `self.lang` only — no status/error
  clear today (hence §2's "new clear" correction).
- `~` is unbound in `keys.rs` (bound punctuation: space, `/`, `?`) — free for
  the diag overlay.
- `toast(` in `touch/app.ts`: 36 call sites (24 modal-lock duplications);
  desktop `ui.ts` has none.
- `promptQuestion()` (`prompt.ts:61-64`) has a **third** fallback —
  `web.prompt.confirmFallback` — now on §4's deletion list (Q13).

**Stated assumption:** the TUI prompt overlay renders its question line from
`PromptView.question` (hosts never reconstruct it); only the legend line
comes from `tui.prompt.<kind>.legend`.

## 12. Second spec review (2026-08-21 grill, round 2)

Eight questions; all recommendations accepted. This round targeted
single-source-of-truth and boundary concerns §11 did not reach.

**Decisions:**

- **Q1** Severity lives in one `severity_of(key)` table in `notice.rs`; call
  sites pass key+args only. §11's phase-1 test then covers every site instead
  of testing a duplicate table. (§2)
- **Q2** `NoticeSource` is developer-facing metadata: it rides the wire and
  feeds the diag ring, and is **never rendered**. No provenance badge or
  prefix. `CONTEXT.md` says so explicitly.
- **Q3** The diag ring taps **every** notice, not just host ones — kind
  renamed `host_notice` → `notice`, recorded in one setter. Amends ADR 0008's
  five-kind list. (§7)
- **Q4** VS Code extension-side native messages are a permanent carve-out,
  now written into §9 with its consequences rather than left as an omission.
- **Q5** `SetHostNotice` carries key+args, not rendered text: `web/i18n.ts`
  shares the repo-root catalog but renders with its own `getLang()`, so
  core-side rendering keeps `Session.lang` the single language authority.
- **Q6** Host notices arrive as `Intent::SetHostNotice`, not a bespoke
  `set_host_notice` setter. `dispatch` (`dispatch.rs:304`) is the sole
  mutation path today — a setter would be the first non-Intent write and
  would reopen the boundary ADR 0003 closed. The Intent carries a
  non-user-action comment.
- **Q7** The 82 Some/None `status`/`error` assertions migrate through
  test-only `error_text()` / `status_text()` helpers (`error_text()` is
  `Some` iff severity is `Error`), so each site keeps its exact original
  meaning. Hand-translation was rejected: under a single slot
  `error.is_none()` is not `notice.is_none()`, and a careless translation
  passes while asserting something weaker. The helpers alone don't exercise
  the *new* behavior (a `Warn` occupying the slot where `error` was `None`
  and `status` was `None`) — Phase 1's verification now adds one explicit
  slot-occupancy test for that case (§8 phase 1 row).
- **Q8** The 7 clipboard-locked touch guards on host operations (no intent
  dispatched) become host notices reusing `core.clipboard.action-locked`;
  §5.2's flat "replace all toasts" is corrected to 17 / 7 / 14.

**Fact verifications** (sub-agent audits, 2026-08-21):

- `Session` derives no `Clone` and is cloned nowhere in core/TUI/FFI
  (`session.rs:19`); undo stores `VecDeque<String>` of serialized documents
  capped at 200 (`state.rs:211-264`), pushed only from `on_mutation_success`
  (`session.rs:1707-1710`). A `VecDeque<DiagEvent>` on `Session` is therefore
  duplicated by nothing. The ring's 256 is an independent budget, not drift
  from undo's 200 (noted in ADR 0008).
- All five `PromptKind` variants (`state.rs:154-173`) carry the args their
  question strings interpolate (`Collision` key; `TypeChange` from/to; the
  other three need none), so §3's per-snapshot rendering is feasible. Note
  the TUI overlay today does **not** read `status` — it re-renders locally
  from the payload via `tr_args` (`ui.rs:652-673`); §11's "stated assumption"
  describes the post-change state, not today's.
- 89 test assertions read `status`/`error`; only 7 compare rendered text
  (`session_headless.rs:787,798,812`, `functional_smoke.mjs:318,323`,
  `schema_headless.rs:805`, `app.rs:1231`). The other 82 are Some/None —
  hence Q7.
- `status_fmt.rs` holds no key→severity mapping (four label formatters,
  `status_fmt.rs:8-57`); `schema::Category` is unrelated. Q1's table is new
  code, and lives in `notice.rs` beside `Severity`.
- `dispatch(&mut self, intent) -> SessionSnapshot` (`dispatch.rs:304-317`) is
  the only mutation path; `Session` has zero `set_*` methods, and the TUI
  writes messages only because the fields are `pub`
  (`app.rs:377,385,454,483,520,525,539,556,678,687,694`) — hence Q6.
- `web/i18n.ts:1-9` imports the repo-root `i18n/en.json` / `zh-TW.json`
  ("the TypeScript twin of `i18n.rs`"), so `t()` already resolves `core.*`
  keys — there is **one** catalog with two renderers, which is why Q5 turns
  on language authority rather than catalog unification.
- Touch toasts: **38** call sites (§11's "36" was low). 24 are
  clipboard-locked and fire `t("core.clipboard.action-locked")`, the same key
  core sets via `guard_clipboard_locked` (`session.rs:658,880,917,1278`,
  `inline_edit.rs:26,802`, `clipboard.rs:14,122`, `undo_redo.rs:11,36`,
  `dispatch.rs:357`) — but 7 of those 24 guard host operations that dispatch
  no intent (`app.ts:695,726,761,888,1047,1231,1549` and keyboard twins
  `1536,1556,1580`), so core sets nothing for them. Hence Q8's 17 / 7 split.
