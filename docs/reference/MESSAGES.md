# MESSAGES.md — the Notice/diagnostics message system, unified across all hosts

The design record is `docs/superpowers/specs/2026-08-21-message-system-design.md`
(15+8-question grill, §11/§12) and ADR 0008 (`docs/adr/0008-in-session-diagnostic-ring-over-tracing.md`).
This document is the reference: the data model, the severity classification table,
per-host channel/rendering behavior, and where the platforms deliberately agree vs.
diverge. `glossary.md`'s "Messages & diagnostics" section stays the one-paragraph
glossary; this is the full detail behind those terms. TUI mechanics beyond the
status line live in `TUI.md` §*Status & diagnostics*; web/touch mechanics beyond
notice rendering live in `WEBUI.md` §*Diagnostics*.

## 1. The model

Two independent, non-overlapping channels carry user-facing text, and a third carries
developer-facing diagnostics. Neither *user-facing* channel is a queue — each holds at most one
live value, replaced (not appended to) by the next write; the diagnostic ring is the exception,
retaining the last 256 events (§1.3).

| Channel | Core field | Audience | Never contains |
|---|---|---|---|
| **Notice** | `Session.notice: Option<Notice>` | End user | Developer-only trace detail |
| **Prompt question** | `ModeView::Prompt.question: String` | End user (inside a modal) | Legend/keybinding text (host chrome) |
| **Diagnostic event** | `Session.diag: DiagRing` (256-entry ring) | Developer | Anything rendered as a Notice |

### 1.1 Notice

```rust
pub struct Notice {
    pub severity: Severity,       // Info | Success | Warn | Error
    pub text: String,             // already localized (Session.lang at write time)
    pub source: NoticeSource,     // Core | HostTui | HostWeb — developer-facing provenance
}
```

(`crates/confy-core/src/session/notice.rs`) One slot on `Session`, mirrored on the
wire as `SessionSnapshot.notice: Option<Notice>`. The next `set_notice` call —
core-internal or host-authored — replaces whatever was showing; nothing queues.
`NoticeSource` is **never rendered** — it exists purely so `diag_log()`/`?diag=1`
can tell "the document rejected your edit" (`Core`) from "the host failed to write
the file" (`HostTui`/`HostWeb`) while debugging, without adding a second
user-visible field hosts would have to design UI for.

**Lifecycle — cleared on:** mutation success (`on_mutation_success`, every
successful `Mutation::apply`), `Esc` (`Session::escape`), entering inline edit
(`begin_inline_edit`/`begin_inline_rename`), and `SetLang` (a **new**, host-only-
retroactive rule this design added — pre-refactor `set_lang` cleared nothing).
Navigation (cursor move, expand/collapse, filter typing) does **not** clear it —
a Notice (any severity, including `Warn`/`Info`/`Success`) persists on screen
until one of the four clearing events above, exactly like the pre-existing
Error-only "never hidden" invariant, just generalized to all four severities.

### 1.2 Prompt question

`ModeView::Prompt { kind, question }` — the question text of an open y/n or
o/r/c prompt (`Collision`, `ConfirmQuit`, `TypeChange`, `ArrayUpgrade` — 4
kinds), rendered **core-side** from `PromptKind` +
`Session.lang` via `prompt_question(lang, pk)` (`dispatch.rs`), one localized
`core.prompt.<kind>` key per kind, legend-free. **Hosts never reconstruct or
parse this text** — the pre-refactor pattern (TUI parsing `status` for prompt
strings, a web `PROMPT_QUESTIONS` fallback table, a `promptQuestion()` strip-legend
hack, and a third `web.prompt.confirmFallback` fallback the original audit missed)
is fully deleted. The y/n or o/r/c **key legend** stays host-chrome, translated
separately per host: TUI reads `tui.prompt.<kind>.legend` inline
(`draw_prompt_overlay`); web/touch read `web.prompt.title.*`/`web.prompt.btn.*`
for their sheet/dialog labels. A prompt question is never written into the
Notice slot and never multiplexed onto `status`/`error` (the pre-refactor two-
bucket model did exactly this, which is why prompt text and status text used to
collide).

A question may carry an extra `note` (today: a TOML datetime retype's `old → new`
preview plus what it drops or fills — ADR 0012), so it can be long, and hosts
must **wrap** rather than clip it: the TUI's `draw_prompt_overlay` sizes its box
from the question's *display* width (`unicode-width`, so double-width zh-TW
glyphs get the rows they need) and renders with `Wrap`. It used to be a fixed
3-row `Paragraph` with no wrap, which silently truncated the tail at the right
edge.

### 1.3 Diagnostic event

Developer-facing, English-only (i18n governs *authored* fragments, not captured
payloads — a `host_notice`/`notice` diag event's `detail` legitimately embeds
whatever localized text a Notice was just set to), never shown as a Notice. See
§4.

## 2. Severity — the single classification table

Four levels, one meaning each:

| Severity | Meaning | Example |
|---|---|---|
| `Info` | Neutral state report | empty/nothing-to-save/cancelled |
| `Success` | Action completed | saved, cut N node(s), converted |
| `Warn` | Action unavailable in current context | readonly, clipboard-locked, precondition-unmet, schema violation |
| `Error` | Operation failed | mutation error, I/O failure, schema load failure |

**Severity is derived from the catalog key, never chosen at the call site.**
`severity_of(key: &str) -> Severity` (`session/notice.rs`) is the single source of
truth for every notice key — there is no explicit-severity
constructor and no escape hatch; a key not yet in the table panics rather than
silently defaulting, so a new Notice call site can't ship unclassified. The table
classifies **68** keys: **45 `core.*`** notice keys (12 Error + 17 Warn + 7 Success
+ 9 Info) and **23** host-authored `tui.*`/`web.*` keys (§3). `core.schema.violation`
is a controller-approved pass-through wrapper (`Warn`) carrying the dynamic
schema-violation advisory text, and is counted inside those 17. See `notice.rs`'s own
`severity_of_covers_the_full_catalog_table` test for the exhaustive `core.*` list;
that test *is* the maintained reference, not duplicated here to avoid drift, and its
`cases.len() == 45` assertion is the tripwire that catches this section going stale.
The host keys are classified by the same table but are deliberately outside that test.

**These 45 are only the notice keys.** `i18n/en.json` holds 102 `core.*` keys in
total; the rest are prompts (`core.prompt.*`), picker and label text
(`core.dt.*`, `core.action.*`, `core.add.type.*`, `core.detail.*`, `core.hint.*`,
`core.comment.advisory`, `core.schema.count`) and would **panic** if passed to
`severity_of` — they are never notices.
Among the host-authored keys (§3):
`web.host.schema.load-error` / `tui.host.schema-load-error` (`Warn`) report a
`$schema` hint that failed to load (local file not found / URL fetch failed) —
dispatched by every host (web desktop, touch, TUI) once
`schema_status.load_error` is set, so the failure is visible outside the VS
Code extension's Problems-panel diagnostic too.

### 2.1 Catalog key prefixes

Flat keys in `i18n/en.json` (canonical) / `i18n/zh-TW.json`, looked up via
`tr(lang, key)`/`tr_args(lang, key, args)` (`confy-core/src/session/i18n.rs`,
en-fallback then raw-key fallback so a missing translation never panics or
blanks the UI):

| Prefix | Origin | Severity-classified? |
|---|---|---|
| `core.*` | `confy-core` internal notices + `core.prompt.*` questions | Notices: yes, via `severity_of`. Prompts: no. |
| `tui.*` | TUI-authored host notices + `tui.prompt.<kind>.legend` chrome + `tui.status.*` mode-hint strings | Host notices: yes (routed through `severity_of` like any other key — see §3). Legend/status-hint strings: no (not Notices at all). |
| `web.*` | Web/touch-authored host notices + `web.prompt.title.*`/`web.prompt.btn.*` chrome | Host notices: yes. Chrome strings: no. |
| `cli.*` | CLI convert-subcommand output/prompts (`crates/confy-tui/src/cli.rs`) | No — the CLI is a one-shot process with no `Session.notice` slot at all (§5.5). |

## 3. Host-authored notices: one dispatch path in, one classification table

A host (TUI, Web desktop, Touch) that needs to report its own event — a save
succeeded, a file write failed, a converted-file write completed — never writes
to `Session` fields directly. It dispatches `Intent::SetHostNotice { key: String,
args: Vec<String>, source: NoticeSource }` (`intent.rs`), keeping `dispatch`
the sole mutation entry point (ADR 0003's TUI-dispatch boundary, reaffirmed
here). `dispatch.rs`'s handler (`dispatch.rs`) resolves severity from the
*same* `severity_of(key)` table core notices use — a host cannot pick its own
severity, and `NoticeSource::Core` claimed by a host is a defensive no-op (never
panics, in any build profile) rather than a trusted self-report:

```rust
Intent::SetHostNotice { key, args, source } => {
    let notice = match source {
        NoticeSource::HostTui => Some(Notice::host_tui(self.lang, &key, &args_refs)),
        NoticeSource::HostWeb => Some(Notice::host_web(self.lang, &key, &args_refs)),
        NoticeSource::Core => None, // hosts never legitimately claim Core provenance
    };
    if let Some(n) = notice { self.set_notice(n); }
}
```

`NoticeSource` is a **closed 3-value enum** (`Core` / `HostTui` / `HostWeb`) —
Tauri desktop, touch, and the VS Code *webview* all ride `HostWeb` (they share
the one Web UI bundle); a new variant is added only if a genuinely distinct
fourth host ships (e.g. a native, non-webview UI). The VS Code *extension host*
is architecturally outside this entirely — see §5.4.

## 4. Diagnostics ring (ADR 0008)

A bounded `VecDeque<DiagEvent>` living on `Session` itself (`session/diag.rs`),
not the `tracing`/`log` crates — kept as ordinary, headlessly-testable data so
`Session` stays a pure, host-free value (`Session` is fully unit-testable and
compiles unchanged for TUI/wasm/VS Code; a global `tracing` subscriber would
fight that). Capacity 256, oldest evicted, monotonic `seq: u64` (never resets,
even across a VS Code file-swap — see the Task 17 review's one Minor finding,
still open as a follow-up, §8).

```rust
pub struct DiagEvent {
    pub seq: u64,
    pub level: DiagLevel,       // Debug | Info | Warn | Error
    pub kind: &'static str,     // "dispatch" | "mutation" | "notice"
    pub detail: String,         // English, structured-ish; may embed captured localized text
}
```

Three kinds, all recorded **unfiltered** (Debug marks navigation noise rather
than dropping it — the ring is meant to answer "what did the user see, in
order", and dropping events would break that for the sake of ring-churn that
costs nothing to keep). There are exactly three `diag.push` call sites in the
whole workspace:

| Kind | When | Level |
|---|---|---|
| `dispatch` | Every `Intent`, first thing in `dispatch()` | Debug |
| `mutation` | After `apply()` — whether the intent's notice slot changed to a fresh Error | Error if a new Error notice just surfaced, else Info |
| `notice` | Every Notice assignment, core-internal or host-authored, via the sole `set_notice` write path — capturing the **rendered text verbatim** plus `severity=`/`source=` | Info |

A host-authored notice is **not** a separate `host_notice` kind: it arrives through
`Intent::SetHostNotice`, lands in the same `set_notice`, and so records as `notice`
with its provenance in the `source=` field. Schema and convert outcomes have no diag
kind of their own today — they are visible only through the `dispatch`/`mutation`/
`notice` events their intents produce.

Capturing rendered text in `notice` events is a deliberate
exception to "diagnostics are English-only": the English-only rule governs
*authored* diagnostic fragments (the `kind`/level/structural wording), not
*captured* payloads — and the rendered text is exactly what i18n debugging
needs to see.

### 4.1 Exports — three surfaces, one ring

| Export | Host | Mechanism |
|---|---|---|
| `~` overlay | TUI | `overlay_diag.rs`'s `draw_diag_overlay` — a centered, read-only popup, per-level color (`Error` red / `Warn` yellow / `Info` cyan / `Debug` dark gray). Host-owned UI state (`App.diag_overlay_open`), not a core `Mode`; `~`/`Esc` closes, mutually exclusive with the language picker. **No scroll state** (Phase 2), but it windows on the ring's **tail** — the last 20 events, further clamped to what the terminal can show — because an operator opens it to see what just happened. The title reports the window (` Diagnostics — last 20 of 25 `) so a clipped view is never mistaken for the whole ring; an empty ring says `(no events yet)` rather than drawing an empty box. |
| `diag_log()` | FFI (any wasm host) | `ConfySession.diag_log()` (`crates/confy-ffi/src/lib.rs`) serializes the whole ring to a JS array via `serde-wasm-bindgen`. |
| `?diag=1` | Web (desktop/touch/VS Code webview) | `drainDiagIfEnabled()` (`web/ui.ts`), called every `render()`. Diffs `session.diagLog()` against a module-level `lastSeenSeq`, printing only newly-recorded events to `console.debug` as `[confy-diag] [LEVEL] KIND DETAIL` — successive interactions log only their own delta, never replay history. Gated behind the query param so it's zero-cost when absent (no console noise in normal use). |

## 5. Per-host channel, behavior, and rendering

### 5.1 TUI

`draw_status` (`crates/confy-tui/src/tui/ui.rs`) renders the single `Notice`
slot alongside mode chrome, in this priority order (highest first — a lower
tier is fully hidden while a higher one is showing):

1. **Error notice** — red background, white bold text, ` ✗ ` prefix. Shown
   outside `Mode::Edit` regardless of any other state (clipboard armed, filter
   active) — the "errors never hidden" invariant.
2. **Active input** — `Mode::Filter`'s inline `/` query field, or `Mode::Edit`'s
   value/name editor. Inside `Mode::Edit` specifically, a pending notice
   *overrides* the edit hints (shown in red with an `(Esc:cancel)` cue) rather
   than being hidden by them — the one tier-2 exception, since a value-commit
   failure has to stay visible right where the user is looking.
3. **Warn/Success/Info notice** — the status bar's default dark-gray slot.
   Also wins over the "clipboard armed" sticky hint (`N node(s) cut —
   v:paste  c/x:toggle  Esc:discard`) and the `FilterResults`-mode tag/count
   line, both of which used to render *first* and silently swallow a pending
   non-Error notice (a real regression, fixed 2026-08-22 — see `CHANGELOG.md`).
   The demotion below tier 2 is rendering-only: the Notice stays in the slot
   and reappears the instant Filter/Edit input exits.
4. **Mode/hint fallback** — clipboard-armed sticky hint, `FilterResults` tag
   line, or the default `pos/total` status with a dynamic schema `edit_hint`
   tooltip, in that order, only once no notice is pending.

`~` opens the read-only diag overlay (§4.1); `l` opens the language picker
(mutually exclusive with `~`, both host-owned UI state outside `Mode`).

### 5.2 Web desktop + VS Code webview

Both ride `NoticeSource::HostWeb` and the same `web/ui.ts` bundle (the VS Code
custom-editor webview is the Web UI with `body.host-vscode` chrome trimming —
not a fourth host). `renderNotice(notice)` (`web/ui.ts`) maps severity to
surface:

| Severity | Rendering |
|---|---|
| `Success` | Toast (auto-hiding) **and** status-bar text (`.sev-success`) |
| `Info` | Status-bar text only (`.sev-info`) |
| `Warn` | Status-bar text only (`.sev-warn`) |
| `Error` | Dedicated red error element, **click-to-clear** (no auto-hide) |

`Error` is the only severity with its own DOM element (`#error`, `.hidden`
toggled) and the only one requiring an explicit user click to dismiss — every
other severity shares `#status`, tinted per severity class, cleared by the next
notice or `notice === undefined` (`Esc`/mutation success/etc., mirrored from
core exactly like the TUI). `?diag=1` drains the ring on every render (§4.1).

### 5.3 Touch

Also `NoticeSource::HostWeb`, but `web/touch/app.ts`'s `renderNotice` is
simpler — one toast element for **every** severity (no separate status-bar
text, no dedicated error element; the smaller viewport has no room for a
persistent status bar the desktop layout affords):

```ts
toastEl.textContent = notice.text;
toastEl.classList.add(`sev-${notice.severity}`); // styling hook, same class names as desktop
const ms = severity === "error" || severity === "warn" ? 3000 : 1600;
```

`Warn`/`Error` get a longer 3000ms auto-hide (vs. 1600ms for `Success`/`Info`) —
touch has no click-to-clear affordance for `Error` the way desktop does, so a
rejection needs enough on-screen time to actually be read before it vanishes.
This is touch's one genuine severity-behavior divergence from desktop's
click-to-clear-forever `Error` treatment, a deliberate small-viewport trade-off
rather than an oversight.

### 5.4 VS Code extension host — a permanent, separate carve-out

The extension host's own native popups
(`vscode.window.showErrorMessage`/`showInformationMessage` in
`editors/vscode/src/editorProvider.ts` — save failures, parse-error recovery
prompts on the native TOML/YAML text editors) are **hardcoded English, outside
the i18n catalog, and never touch `Session.notice`, the diag ring, or
`NoticeSource` at all**. This is intentional, not a gap: the extension host
runs before/outside any webview's JS context (native-editor save/parse errors
can fire with no confy webview open), VS Code's own `l10n` mechanism — not
confy's `Lang`/catalog — is the idiomatic localization path for genuinely
native extension-host UI, and the population is small (two call sites). If VS
Code ever needs more than these two prompts, route new ones through the same
native `vscode.window.show*Message` API and accept the same English-only,
un-diagnosed status — **not** a bridge back into core's Notice slot, which
would require plumbing a webview-less document's `Session` state across a
boundary that doesn't otherwise need one.

### 5.5 CLI

`confy convert <in> <out>` is a one-shot, non-interactive-capable process, not
a `Session` with a rendered UI loop — it has **no** Notice slot, no diag ring,
and no severity concept. Every user-facing string (`cli.*` catalog keys:
convert warnings/prompts, file-creation confirmation, error diagnostics) is
looked up via `tr`/`tr_args` directly and printed to stdout/stderr or read from
stdin (`crates/confy-tui/src/cli.rs`), with `--lang` resolved through the same
config-file precedence chain the TUI's language picker uses (`--lang` flag >
config file > `en`). This is a deliberate, permanent scope boundary — the CLI's
job is exit codes and process-lifetime text, not the multi-turn transient
message model the rest of this document covers.

**Testing localized CLI output.** Because the language falls back to the *real*
`~/.config/confy/config.toml` when no flag is given, a CLI integration test that
asserts message text **must pin `--lang en`** (or derive the expected string via
`tr`/`tr_args` for the language it does pin). An unpinned English assertion reads
developer machine state: it passes in CI and fails for anyone whose config sets
`lang = "zh-TW"`. To test the *precedence chain itself*, isolate the config file
with `.env("XDG_CONFIG_HOME", tmpdir)` instead of relying on the ambient one —
`convert_cli_respects_config_file_lang_when_no_flag` in
`crates/confy-tui/tests/convert_cli.rs` is the reference pattern.

## 6. Unified design principles

- **One classification table, no per-call-site severity.** §2's `severity_of`
  is the single source of truth for both core-internal and host-authored
  notices — a host cannot invent its own severity for a key, and an
  unclassified key panics rather than shipping silently miscategorized.
- **One mutation path.** Every write to the Notice slot — core-internal
  (`set_notice`) or host-authored (`Intent::SetHostNotice`) — goes through
  `Session::dispatch`, preserving the pre-existing TUI-dispatch boundary
  (ADR 0003) instead of adding a second, bespoke setter.
- **One provenance concept, developer-only.** `NoticeSource` exists solely to
  make `diag_log()`/`?diag=1` legible during debugging; it is never rendered
  to the end user on any host, and hosts cannot claim `Core` provenance for
  their own notices.
- **One clearing lifecycle for every severity.** §1.1's four clearing events
  (mutation success / Esc / edit begin / language switch) apply uniformly —
  there is no severity-specific "auto-expire" at the core level; where a host
  auto-hides (touch's toast timers, desktop's Success toast), that's a
  host-local rendering choice layered on top of a core state that itself never
  times out.
- **Additive wire contract.** `SessionSnapshot.notice: Option<Notice>` replaced
  the legacy `status: Option<String>` / `error: Option<String>` pair in a
  single paired core+web cutover (dual-write mitigation across Phase 1–3,
  now fully complete — the legacy fields no longer exist anywhere in the
  codebase); no host had to carry a transitional dual-read branch permanently.
- **Prompt question is never a Notice.** §1.2's core-rendered, legend-free
  question string is a structurally separate wire field
  (`ModeView::Prompt.question`), closing the pre-refactor pattern where prompt
  text and status text shared one field and periodically collided.

## 7. Adjacent channels this document does not cover

Two more user-facing text mechanisms exist in the codebase, entirely outside
the Notice/Prompt-question/Diagnostic-event model above — noted here (closing
a gap `docs/superpowers/plans/2026-08-28-json-jsonc-parser-simplification-ssot.md`
found: neither had a home in this document) so a future audit doesn't mistake
their absence above for an oversight in §1's channel inventory, which is
scoped to the Notice-system model only.

### 7.1 Comment advisory (`ViewRow.comment_advisory`)

A **persistent per-row projection**, not a Notice: `Option<String>`, plain
translated text, no `Severity`, no `NoticeSource`, recomputed on every row
rebuild alongside the rest of `ViewRow` rather than written through
`set_notice`/`Intent::SetHostNotice`. See `glossary.md`'s "Comment advisory"
glossary entry for its trigger condition (`Session.strict_json` + node
facets). Because it lives on the row, not in the single-slot `Session.notice`
field, it cannot collide with or be cleared by anything in §1.1's Notice
lifecycle — the two mechanisms happen to render near each other in some UIs
but never contend for the same storage.

### 7.2 Convert warnings (`ConvertResult.warnings`)

A `Vec<String>` of **raw, untranslated** English strings returned by
cross-format convert/save-as (lossy-conversion notes — e.g. "comments will be
dropped", "duplicate key merged"). Rendered as an ad hoc bullet list by each
host's own convert-confirm surface (CLI stderr, TUI's `overlay_convert`, web's
convert-dialog) rather than through `tr`/`tr_args` — this channel bypasses
i18n entirely, unlike every other user-facing string in this document.
Unifying it into the catalog/Notice model is a known, explicitly out-of-scope
follow-up (not tracked by an issue as of this writing).

## 8. Known follow-ups (non-blocking, recorded for later)

Re-verified 2026-09-09 and still open. They are tracked as **F4/F5/F7/F8** in
[`../plan/2026-09-09-open-follow-ups.md`](../plan/2026-09-09-open-follow-ups.md), the single
live backlog, which carries their acceptance criteria; the evidence is in
[`../audit/2026-09-09-open-findings-reverification.md`](../audit/2026-09-09-open-findings-reverification.md).

- **Web `?diag=1`'s `lastSeenSeq` doesn't reset on a file swap.** Opening a new
  file replaces the underlying `ConfySession` (and therefore its diag ring, which
  restarts its `seq` from 0) without resetting `web/ui.ts`'s module-level
  `lastSeenSeq` counter — the post-swap events with `seq` below the old
  high-water mark are silently skipped in the console drain until `seq` catches
  back up. `lastSeenSeq` is advanced only inside `drainDiagIfEnabled` and reset
  nowhere, and **8** paths replace the session, so the reset belongs next to a
  single session-replacement helper rather than at each site. Touch has no drain
  at all — the `?diag=1` trace is desktop-only. Benign: it affects only the
  debug-only console trace, never the ring itself or any user-visible surface.
- **In the TUI, the only Intent that reaches `dispatch` is `SetHostNotice`.**
  Measured 2026-09-09 on the real binary: 34 navigation keystrokes left the ring
  **empty**, and every event that does appear arrives as the same fixed triple
  (`dispatch SetHostNotice` / `notice …` / `mutation SetHostNotice ok`). The
  channel therefore records *messages the host already displayed* and nothing
  else — no navigation, no mutation, no mode change — because ~15-20 sites in
  `tui/app.rs` and `tui/mod.rs` mutate `Session` fields directly instead of
  dispatching. A `dispatch` line that always names the same Intent is not a
  trace. Tracked as **F7**; the `~` overlay's tail-take fix (2026-09-09) made
  the window useful, it did not make the channel complete.
- **Touch `sev-*` toast classes have no dedicated CSS yet.** `web/touch/app.ts`'s
  `renderNotice` applies the classes (§5.3) but `web/touch/style.css` has
  `.toast`/`.toast.show` and zero `sev-*` rules, so a `Warn` differs from a
  `Success` only by its auto-hide timer (3000 ms vs 1600 ms). Note `web/style.css`
  *does* style `sev-warn`/`sev-success` — for the desktop footer status line, not
  the toast — so the two hosts currently disagree on whether severity is visible
  at all. Cosmetic, deferred, MVP-scope.
- **One mistake, two severities: JSON reports a bad fragment as `Illegal`,**
  TOML and YAML as `Fragment`. Measured 2026-09-09 on a value `Replace` of an
  unterminated string: TOML `Fragment("unexpected token")`, JSON
  `Illegal("expected R_BRACE, found None")`. §2 maps the two variants to
  different severities, so the user sees a different message class for the same
  typo depending only on which format the file is in. This is the *fragment*
  half of the variant confusion; the *gesture-does-not-apply* half was unified
  to `Unsupported` across all three backends on 2026-09-09 (`72805c0`) — by
  hand, per backend, which is the work **F8** (`MutateError` taxonomy) exists to
  make unnecessary. Fix it there, not per-backend. Note the YAML row is not
  simply "missing validation": its subset lexer accepts an unterminated scalar
  *on load* too, so `Replace` and load agree; see the backlog's *Watching*
  section before tightening either.
