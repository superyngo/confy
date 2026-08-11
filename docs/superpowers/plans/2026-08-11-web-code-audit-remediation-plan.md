# Web Code Audit Remediation — Implementation Plan

Date: 2026-08-11
Source: code-auditor skill session, 2026-08-11 (chat-delivered report, not persisted as a
separate audit doc — this plan is the durable record).
Scope: `confy/web` only (desktop + touch web UI). 8 tasks, 2 phases.

Note: an unrelated, already-fully-shipped `2026-08-11-audit-remediation-plan.md` exists in this
same directory (different audit, `docs/superpowers/audits/2026-08-11-optimization-organization-integration-cleanliness-audit.md`,
covers Rust core/TUI plus a handful of web items). This plan is deliberately filed under a
distinct name and does not touch that plan's scope, except where noted (Task 5 below revisits
one item that plan already reviewed once).

## Overview

Turns the code-auditor report's findings into discrete tasks. The audit's top finding: strict
TypeScript (`tsconfig.json`'s `strict: true`) and the one existing regression test
(`toolbar-fold.spec.mjs`) are never enforced anywhere — traced every workflow in
`.github/workflows/`; none are push/PR-triggered, only tag-push/`workflow_dispatch`/`workflow_run`.
A broken `web/` change can merge to `main` and only surface at release time. Everything else
(dedup, test-coverage gaps, stray debug logging) is secondary to closing that gap.

## Architecture Decisions

- **CI mechanism: `web-ci.yml` reuses `cf-build.sh` via the existing composite action, not a
  separate step list.** `tsconfig.json`'s `include` references `../crates/confy-ffi/pkg/*.d.ts`,
  which is gitignored and only exists after `wasm-pack build --target web` runs — so a
  "wasm-free" typecheck workflow is impossible as originally scoped (confirmed: fresh checkout
  has no `crates/confy-ffi/pkg/`). Since Task 2 puts `npm run typecheck && npm test` inside
  `cf-build.sh` itself, Task 1's workflow collapses to "run
  `.github/actions/build-web-frontend` (`build-frontend: true`)" — one script, no drift between
  the PR-time check and the release gate. Task 1 therefore **depends on Task 2**.
- **No GitHub branch protection.** `confy` is a single-committer repo (`git shortlog`: 486/486
  commits by one author, 4 merge commits in the last 50) — direct `git push` to `main` is the
  actual workflow, not PRs. GitHub's required-status-checks feature blocks direct pushes too,
  but only for a commit SHA that *already* has a passing check recorded — a `push`-triggered
  workflow can't satisfy that for its own triggering push, so turning this on would reject the
  maintainer's normal `git push origin main` outright (`GH006`). `web-ci.yml` is therefore
  **visibility only** (a pass/fail signal on the commit); the actual enforcement is Task 2's
  synchronous gate inside `cf-build.sh`, which nothing ships (release/CF Pages deploy) without
  passing regardless of how the commit reached `main`.
- **`batch()` dedup is a factory extraction, not a verbatim move.** `ui.ts:838-848`'s `batch()`
  calls `render(); notifyHost();` in `finally`; `touch/app.ts:128-137`'s calls `render();` only
  (touch has no VS Code bridge). Not byte-identical — Task 4 extracts a small
  `createBatcher(render, afterRender?)` factory each host instantiates with its own hook, not a
  plain cut-paste. `modeTag` (`ui.ts:394-396` / `touch/app.ts:118-120`) *is* byte-identical and
  moves cleanly (Task 3).
- **`replaceSession` stays untested this round.** `host-io.ts:109-124` calls the real wasm
  `Session.fromText()` (`confy.ts` → `pkg/confy_ffi.js`, wasm-pack `--target web`). Loading that
  in plain Node needs a `fetch(file://…)` shim or a different wasm-init path — real
  infrastructure work, and the existing test convention (`toolbar-fold.spec.mjs`'s own comment)
  deliberately avoids new dependencies like jsdom. Every other `host-io.ts` flow function
  (`doQuickSave`, `doSaveAsCopy`, `doConvertWrite`, `openFromUrl`) only touches the *injected*
  `HostIo` interface (`host-io.ts:24-57`) and is fully mockable without wasm — those are Task 7's
  scope. `replaceSession` coverage is a named follow-up, not silently dropped.
- **Trigger paths cover `crates/confy-ffi/**` too, not just `web/**`.** A Rust-side signature
  change in `confy-ffi` can silently break the web TS layer (wrong `.d.ts` shape) without
  touching any file under `web/` — `web-ci.yml`'s `push`/`pull_request` `paths` filter includes
  both.

## Phase 1 — Quick wins (< 1 day)

### Task 1: Push/PR-triggered CI workflow for `web/` (visibility, not a merge gate)
- **File**: new `.github/workflows/web-ci.yml`
- **Description**: Fast pass/fail feedback on every push/PR touching `web/**` or
  `crates/confy-ffi/**`. Not a merge gate (see Architecture Decisions) — the release build
  (Task 2) is the actual enforcement.
- **Details**:
  - Trigger: `on: { push: { paths: ["web/**", "crates/confy-ffi/**"] }, pull_request: { paths: ["web/**", "crates/confy-ffi/**"] } }`
  - Steps: checkout → `.github/actions/build-web-frontend` with `cache-key: "web-ci"`,
    `build-frontend: "true"` (default). That installs the Rust/wasm32 toolchain, builds
    `crates/confy-ffi/pkg/` via `wasm-pack`, then runs `bash web/cf-build.sh`, which (once Task 2
    lands) already runs `npm ci && npm run typecheck && npm test` before assembling `dist/`. No
    separate step list.
  - No GitHub branch protection / required status check is configured.
- **Dependencies**: **Task 2** — this workflow's entire test/typecheck value lives inside
  `cf-build.sh`'s new gate step; landing Task 1 first would just re-run a build with no checks.
- **Verify**: Push a branch with a deliberate type error in `web/*.ts` — workflow fails; revert,
  confirm it passes. Cannot run a live GitHub Actions job in this environment beyond a manual
  YAML review + `actionlint` if available.

### Task 2: Typecheck + test gate in `cf-build.sh`
- **File**: `web/cf-build.sh:24-25`
- **Description**: Release/store builds fail fast on a type error or test regression, not just
  PR-time changes.
- **Details**: After line 24 (`( cd crates/confy-ffi && wasm-pack build --target web )`), add
  `( cd web && npm ci && npm run typecheck && npm test )`, before the existing line 25
  (`( cd web && npm install && node build.mjs )`) — or fold both `cd web` blocks into one,
  matching the file's existing one-command-per-step style.
- **Dependencies**: None (Task 1 depends on this task, not the reverse — see Architecture Decisions).
- **Verify**: `bash web/cf-build.sh` run locally (needs `cargo`/`wasm-pack` on PATH) completes
  through the new step with 0 errors; introduce a deliberate failure locally to confirm `set -euo
  pipefail` (line 8) aborts the build.

### Task 3: Extract `modeTag` into a shared module
- **File**: new `web/mode.ts`; edit `web/ui.ts:394-396`, `web/touch/app.ts:118-120`
- **Description**: Byte-identical function, currently duplicated.
- **Details**:
  - `mode.ts`: `export function modeTag(m: ModeView): string { return typeof m === "string" ? m : Object.keys(m)[0]; }`
    (import `ModeView` from `./types.js`).
  - Both call sites: delete the local definition, add `import { modeTag } from "./mode.js";`
    (touch: `"../mode.js"`).
- **Dependencies**: None.
- **Verify**: `npm run typecheck` clean; `grep -rn "function modeTag" web/` finds only `mode.ts`.

### Task 4: Factor `batch()` into a shared `createBatcher` helper
- **File**: `web/mode.ts` (same file as Task 3); edit `web/ui.ts:837-848`,
  `web/touch/app.ts:121,123-125,127-137`
- **Description**: Same batching-flag/try-finally shape, different post-render hook per host.
- **Details**:
  - Add to `mode.ts`:
    ```ts
    export function createBatcher(render: () => void, afterRender?: () => void) {
      let batching = false;
      return {
        isBatching: () => batching,
        batch(fn: () => void) {
          if (batching) return fn(); // nested batches render at the outermost level
          batching = true;
          try {
            fn();
          } finally {
            batching = false;
            render();
            afterRender?.();
          }
        },
      };
    }
    ```
  - `ui.ts`: replace `let batching` / `function batch` with
    `const { batch, isBatching } = createBatcher(render, notifyHost);` at the same point in the
    module (confirm `render` is already defined at that point — it is, `render()` appears
    earlier in the file per the audit's complexity scan).
  - `touch/app.ts`: `const { batch, isBatching } = createBatcher(render);` (no `afterRender`).
    `touch/app.ts:123-125`'s `send()` reads module-level `batching` directly
    (`if (!batching) render();`) — replace with `isBatching()`.
  - Before deleting the module-level `batching` variable in either file, `grep -n "batching"
    web/ui.ts web/touch/app.ts` to confirm no other reader is missed.
- **Dependencies**: Task 3 (same new file).
- **Verify**: `npm run typecheck` clean; manual smoke via `npm run serve` — multi-intent actions
  (e.g. multi-select delete) still re-render exactly once on both `index.html` and
  `touch.html?ui=touch`; VS Code webview build (`editors/vscode`) still receives `notifyHost()`
  after a batched dispatch (grep its dirty-state consumer if unsure which behavior to check).

### Task 5: Resolve the stray debug `console.log`s
- **File**: `web/touch/app.ts:1254,1260` (`openOpenedUrl`)
- **Description**: Two `console.log` calls (`"[confy] opened url:"` / `"[confy] opened name:"
  ... text head:"`) print on every Tauri opened-file event. **History check before acting**: the
  sibling `2026-08-11-audit-remediation-plan.md` already reviewed this exact code once (its own
  Task 2), when it sat behind a comment marked "remove once the content:// read bug is
  diagnosed." That comment is gone today — replaced by the current substantive dedup-rationale
  comment at `touch/app.ts:1244-1249` — but the two `console.log` calls themselves were kept,
  meaning that plan's fallback ("if unresolved, downgrade to: leave in place, drop the stale
  comment") is what actually happened, not a straight removal.
- **Details**: `CHANGELOG.md` traces the `content://` Android read/dedupe work through M1/M2
  (lines ~137-201, ~231-235) as shipped and stable, and the `openedUrlsHandled` dedup set these
  logs sit inside (`touch/app.ts:1250`) is itself the landed fix. No open GitHub issue (`gh
  issue list` search: 0 results) tracks the underlying Android bug. Settled: **delete both
  `console.log` calls outright** — no active reason found to keep them, don't re-decide this a
  third time.
- **Dependencies**: None.
- **Verify**: `grep -n "console.log" web/touch/app.ts` — 0 matches in `openOpenedUrl`; `npm run
  typecheck` clean.

**Phase 1 testing strategy**: no new tests needed — mechanical/config changes covered by
`npm run typecheck` (checked locally pre-push; `web-ci.yml` (Task 1) surfaces it in CI once
Task 2 lands, but doesn't gate the push — see Architecture Decisions) and manual smoke
after every task in this phase.

## Phase 2 — Medium-term (1-5 days)

### Task 6: Tests for `render.ts` row/value HTML escaping
- **File**: `web/render.ts:89` (add `export` to `renderRow`); new `web/render.spec.mjs`
- **Description**: Pin the escaping discipline verified manually in the audit — a config
  key/value/comment containing `<`/`&`/`"` must render escaped.
- **Details**:
  - `render.ts`: add `export` to `function renderRow(...)` (line 89) — currently module-private;
    no behavior change.
  - New test file follows `toolbar-fold.spec.mjs`'s convention (`esbuild.transform` +
    `node:assert`, no framework/jsdom): build fixture `ViewRow` objects (`types.ts:34-55`) with
    `key`/`value`/`trailing_comment` containing `<script>alert(1)</script>` and `&"'`, call
    `renderRow(r, 0, [r], null, null, "")`.
  - Assertions: output `.includes(escapeHtml(payload))` for each injected field; never contains
    the raw unescaped payload.
  - Also test `panelHTML` (`panel.ts:54`, already exported) with the same hostile payload in
    `value`/comment fields.
- **Dependencies**: None.
- **Verify**: `node web/render.spec.mjs` exits 0; temporarily remove one `escapeHtml()` call in
  `render.ts` locally to confirm the new test actually fails (proves it's not a vacuous
  assertion), then revert.

### Task 7: Tests for `host-io.ts` save/open/convert flows
- **File**: new `web/host-io.spec.mjs`
- **Description**: Exercise `doQuickSave`, `doSaveAsCopy`, `doConvertWrite`, `openFromUrl`
  against a fake `HostIo` + fake `FsHandle`/`FsWritable` — no wasm, no DOM.
- **Details**:
  - Fake `HostIo` satisfying `host-io.ts:24-57`: in-memory `serialize()`/`getSnap()`/
    `getHandle()`/`setHandle()` backed by test-scoped state; `send`/`batch` recording calls;
    `ok`/`err` pushing to arrays for assertion.
  - Fake `FsHandle`: `createWritable()` returns a fake `FsWritable` recording `write(text)`
    calls.
  - Cases:
    - `doQuickSave` (`host-io.ts:166-203`): no handle → falls back to the Save-As-equivalent
      path; existing handle → writes in place, calls `ok`, never calls `adoptFile`.
    - `doSaveAsCopy` (`host-io.ts:223-246`): writes serialized text verbatim, calls `adoptFile`
      with the new handle/name.
    - `doConvertWrite` (`host-io.ts:253-282`): calls `beforeConvertWrite?.()` when present,
      picks Save-As vs. `downloadText` fallback per `HostIo.canSaveAs`/`fsAvailable`, calls
      `adoptFile` on success.
    - `openFromUrl` (`host-io.ts:129-147`): stub global `fetch` (Node 20 built-in, no new
      dependency) with canned text/content-type; assert `formatFromNameOrType` picks the right
      `ConfigFormat`; assert the failure path calls `io.err(...)` and returns `false` without
      calling `openText`.
  - Exclude `replaceSession` (see Architecture Decisions) — leave an explanatory comment in the
    spec file.
- **Dependencies**: None (runs independently of Task 6).
- **Verify**: `node web/host-io.spec.mjs` exits 0.

### Task 8: Extract a pure key-resolution function from `onKey`, then test mode-precedence
- **File**: `web/ui.ts:618-823`; new `web/key-intent.ts`; new `web/key-intent.spec.mjs`
- **Description**: `onKey` mixes pure "which Intent does this (mode, key) pair mean" logic with
  side effects (`ev.preventDefault()`, `$("search").focus()`, `uiUndo()`/`uiRedo()`). Extract the
  pure part so mode-precedence (Edit > Prompt > Convert > TypeFilter > KindSwitch > SchemaEnum >
  Help > tree shortcuts) is unit-testable without a DOM, mirroring `toolbar-fold.ts`'s existing
  pure-logic-extraction pattern.
- **Details**:
  - `key-intent.ts` exports `resolveKeyIntent(mode: ModeView, key: string, mods: { ctrl: boolean;
    shift: boolean }, rawView: boolean): { intent: Intent } | { native: "focus-search" | "undo" |
    "redo" } | null` — a pure switch mirroring `ui.ts:618-823`'s branch structure. The three
    branches that call out to host functions (`$("search").focus()` at line 814, `uiUndo()`/
    `uiRedo()` at lines 805-806) return the tagged `native` sentinel instead.
  - `onKey` becomes a thin wrapper: compute `ctrl`/`shift`/mode, call `resolveKeyIntent`, switch
    on the result to call `send(...)` / `ev.preventDefault()` / `$("search").focus()` /
    `uiUndo()` / `uiRedo()` as before. Pure refactor — behavior must be unchanged.
  - Test file: table-driven cases, at least one per guard clause in `ui.ts:618-822`, e.g. `{ Edit
    mode + "Enter" → EditCommit }`, `{ Prompt mode + "y" → PromptKey:"y" }`, `{ Convert/Format
    step + ArrowDown → ConvertMove:1 }`, `{ Help mode + "j" → null (tree shortcuts suppressed) }`,
    `{ ctrl + non-s/o key → null }`, `{ rawView + "j" → null }`, `{ shift+ArrowDown →
    ExtendSelectDown }`.
- **Dependencies**: None, but land after Tasks 6-7 so `npm test` (gated by Tasks 1-2) has a
  substantive suite before those tasks' spec files exist — sequencing preference, not a hard
  blocker.
- **Verify**: `node web/key-intent.spec.mjs` exits 0; manual exercise of every mode transition in
  a running dev build (`npm run serve`) after the refactor — not just the new unit tests — before
  merging, since this is the highest-risk task in the plan (touches the primary input-handling
  path).

**Phase 2 testing strategy**: `package.json`'s `test` script (`package.json:11`, currently `node
toolbar-fold.spec.mjs`) must become a runner over all `*.spec.mjs` files once Tasks 6-8 land,
e.g. `for f in *.spec.mjs; do node "$f" || exit 1; done`. After all of Phase 2 lands, re-run the
full gate: `npm run typecheck`, `npm test`, manual desktop+touch smoke (open a sample doc,
navigate, edit every field type, open every overlay/popup on both `index.html` and
`touch.html?ui=touch`).

## Cross-cutting Integration Points

- Tasks 3-4 touch `ui.ts`/`touch/app.ts` module-level `batching` state — run `lsp references` on
  `batching` in both files before deleting the local declarations.
- Task 8 is the highest-risk task in this plan (extracts the primary keyboard-input dispatch) —
  land last, after Tasks 3-7 have established the multi-spec-file test runner (Phase 2's testing
  strategy), and verify with a full manual pass, not just its own unit tests.
- `web-ci.yml` (Task 1) depends on Task 2's gate step existing inside `cf-build.sh` to have any
  test/typecheck value — land Task 2 first, or in the same change.
- No branch protection is configured on `main` (see Architecture Decisions) — don't re-add
  required-status-check protection without re-checking the direct-push catch-22 for this
  single-committer repo.

## Metrics / Definition of Done

- `npm run typecheck` (`web/`): 0 errors — enforced by the release build gate (Task 2, inside
  `cf-build.sh`); surfaced as a visible pass/fail signal in CI (Task 1) on every push/PR, not a
  merge gate.
- `npm test` (`web/`): all `*.spec.mjs` files passing — same enforcement split as above.
- `modeTag` defined once (`web/mode.ts`), zero duplicate definitions.
- `batch()` behavior unchanged, defined via one shared factory.
- 0 `console.log` calls left in `touch/app.ts`'s `openOpenedUrl` (Task 5).
- New coverage: `render.ts`/`panel.ts` HTML-escaping (Task 6), `host-io.ts` save/open/convert
  flows minus `replaceSession` (Task 7), `onKey` mode-precedence via `key-intent.ts` (Task 8).
