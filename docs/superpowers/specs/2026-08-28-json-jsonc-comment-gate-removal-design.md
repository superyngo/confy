# JSON/JSONC comment write-gate removal — Design

✅ **Shipped — historical reference.** See `CHANGELOG.md` for current behavior; this design record is kept for context, not as a live specification.

Status: approved for planning (design phase)
Date: 2026-08-28
SSOT: `docs/superpowers/plans/2026-08-28-json-jsonc-parser-simplification-ssot.md`
(background investigation + the three consensus decisions this design
implements). Motivated by
`docs/superpowers/plans/2026-08-28-comment-advisory-followup-issues.md` issue #1.

## Goal

Remove the JSON/JSONC write-permission gate (`comments_enabled` /
`supports_comments()` / the `JsoncUpgrade` interactive prompt) so that
authoring a comment into any `.json` document is always mechanically legal —
identical to how TOML/YAML already behave — and is signaled to the user
*only* through the existing per-row `comment_advisory` projection, at load
time and at author time alike, with no special-cased "did the user already
accept JSONC" session state to fall out of sync.

## Non-goals (this pass)

- **YAML/TOML.** Both already have no write gate (`supports_comments()`
  always `true`). Untouched.
- **`comment_advisory` recompute correctness beyond removing the gate.**
  Issue #1 (`2026-08-28-comment-advisory-followup-issues.md` §1) may still
  require its own fix if `to_view_row` doesn't re-derive `comment_advisory`
  on every rebuild for reasons unrelated to the gate. This design removes the
  gate/prompt special case that issue #1 suspects of causing the missed
  recompute; a passing regression test (Task in the plan below) is the
  acceptance bar, not a promise about root cause.
- **Followup issue #2** (advisory vs. schema-hint fighting on load) — a
  `Session.notice` single-slot clearing-order issue, orthogonal to this gate
  removal. Not addressed here.
- **Followup issue #3** (insert splitting a node from its trailing comment) —
  a per-format CST-edit anchor bug, unrelated to the write gate. Not
  addressed here.
- **Any new one-shot toast for authoring.** Per SSOT §4 consensus: no
  compensating `Session.notice` is added when a comment is authored on a
  previously-clean `.json`; `comment_advisory` is the sole signal.
- **`MESSAGES.md` updates** documenting the `comment_advisory` /
  `ConvertResult.warnings` channels. Out of scope (SSOT §5/§7).

## Current behavior (what's being removed, verified against source)

Three call sites gate on `JsonDocument.comments_enabled` /
`ConfigDocument::supports_comments()` (the grill round after the first draft
of this doc found the third one and corrected the first two's file
attribution):

- `Session::add_comment_sibling` (`crates/confy-core/src/session/inline_edit.rs:906-914`) —
  hard-blocks with `Notice::core(self.lang, "core.comment.unsupported", &[])`
  when the gate is closed.
- `Session::remark` (`crates/confy-core/src/session/clipboard.rs:434-463`) —
  when the cursor targets a non-comment node (`authoring == true`) and the
  gate is closed, enters `Mode::Prompt(PromptKind::JsoncUpgrade { pending:
  PendingComment::Remark { path } })` instead of calling `do_remark`
  directly. The `'y'/'Y'` accept branch lives in `session.rs:1965-1978`
  (calls `enable_comments()` then replays `do_remark`); `dispatch.rs:562-564`
  and `:588-589` only hold the `PromptKind -> PromptView` / message-key
  projection, not the gate check itself.
- `Session::split_value_comment` gate at `session/inline_edit.rs:366-368` —
  `.filter(|d| d.supports_comments())` before splitting a trailing `//`/`/*
  */` comment off an edited value. Not part of either flow above; found only
  by grepping every `supports_comments()` caller, not by tracing the two
  known UI actions.

`comments_enabled` becomes `true` from three independent triggers today
(`crates/confy-core/src/model/json/doc.rs:130-235` region, content-based
detection at `from_str`; host-driven `enable_comments()` for a `.jsonc`
extension; the prompt above) — all three collapse away with the gate itself.

**A fourth reader, not a gate — must survive as a distinct fact.** Both
`confy-tui/src/tui/mod.rs:49-58` and `web/ui.ts:289-302` read
`supports_comments()` at load time to decide whether to fire the one-shot
"json-comments-detected" toast (comments already present when a plain
`.json` was opened). This reuses the write-permission boolean as a
content-had-comments-at-open fact — those are two different meanings living
in one field today. See §*Content-only replacement for the load-time toast*
below; this reader is **not** deleted, it's re-pointed.

## Architecture

Three layers, top-down removal — `confy-core` first (matches the "start from
the most basic parser" ordering), then the two thin wrappers around it.

### 1. `confy-core` — drop the gate, drop the prompt, split off the toast fact

**`model/json/doc.rs`** (`JsonDocument`):
- Delete the `comments_enabled: bool` field and `enable_comments()`.
- Add `pub(crate) had_comments_at_open: bool`, set once at `from_str` from
  the exact same content-based lex check `comments_enabled` used today
  (`crates/confy-core/src/model/json/doc.rs:149-159` region) — this is the
  *only* survivor of the old field; it is never written to after
  construction (no `enable_*` setter — there is nothing left to "enable").
- `ConfigDocument::supports_comments()` is deleted from the trait
  entirely — not hardcoded `true`. `TomlDocument`/`YamlDocument`'s
  always-`true` impls are deleted too (`model/cst_doc.rs:78-80`,
  `model/yaml/doc.rs:63-65`), along with the trait method declaration
  (`model/document.rs:28-29`) and `AnyDocument`'s delegating impl +
  its dedicated test (`model/any_doc.rs:98-101,203-206`).
- Add a new trait method `had_comments_at_open(&self) -> bool` (default
  `false` on the trait, or per-backend — `TomlDocument`/`YamlDocument`
  return `false` unconditionally since neither has a "surprise, this
  format silently accepted a foreign notation" story; only
  `JsonDocument` returns its real field). This is the sole replacement
  reader for the two host load-time toasts (see below) — it is a content
  fact, never a write permission, and nothing ever flips it after load.

**`session/inline_edit.rs`**:
- `add_comment_sibling` (current lines 906-914 gate, full function
  906-943):
  ```rust
  fn add_comment_sibling(&mut self, target: Target) {
      let doc = match self.doc.as_mut() {
          Some(d) => d,
          None => return,
      };
      // supports_comments() check + core.comment.unsupported notice: REMOVED.
      // A leading blank line keeps the new comment a *separate* single-line node
      // instead of merging into the adjacent comment (consecutive `#` lines
      // project as one node; a blank splits them).
      let text = format!("\n{} ", doc.comment_prefix());
      // ...unchanged from here down...
  }
  ```
- `split_value_comment` gate at lines 366-368 — delete the
  `.filter(|d| d.supports_comments())`; the call becomes
  `self.doc.as_ref().map(|d| d.split_value_comment(&raw_value))`
  unconditionally, matching TOML/YAML (which never had this filter to
  begin with, since their `supports_comments()` was always `true`).

**`session/clipboard.rs`** (`remark`, lines 434-463) and **`session.rs`**
(prompt-accept handling, lines 1965-1978):
- In `remark`, delete the `supports`/`authoring && !supports` block
  (lines 451-461); the function becomes `cursor_is_read_only` check ->
  resolve `path` -> `self.do_remark(path)` directly, same shape as
  TOML/YAML's `remark` already has (no gate to check).
- Delete the `Mode::Prompt(PromptKind::JsoncUpgrade { .. })` match arm in
  `session.rs:1965-1978` (the `'y'/'Y'` accept branch and the reject
  fallthrough) — nothing constructs this `Mode` variant anymore.

**`session/state.rs`**:
- Delete `PromptKind::JsoncUpgrade { pending: PendingComment }`
  (`state.rs:167-169`) and `PendingComment` (`state.rs:172-174`) — the
  enum has exactly one variant (`Remark { path }`) and no caller once the
  `remark`/`session.rs` sites above are gone.

**`session/dispatch.rs`**:
- Delete the `PromptKind::JsoncUpgrade => PromptView::JsoncUpgrade`
  projection arm (`dispatch.rs:563`) and the
  `PromptKind::JsoncUpgrade => tr_args(lang, "core.prompt.jsonc-upgrade", &[])`
  message-key arm (`dispatch.rs:589`).

**`session/view.rs`**:
- Delete `PromptView::JsoncUpgrade` (`view.rs:155`).

**i18n catalogs** (`i18n/en.json`, `i18n/zh-TW.json`) and
`session/notice.rs::severity_of` (`crates/confy-core/src/session/notice.rs:45-77`,
`:102-106` table):
- Delete keys: `core.comment.unsupported`, `core.prompt.jsonc-upgrade`.
- Delete the corresponding `severity_of` table entries for those keys (they
  currently must be registered or `severity_of` panics on lookup — deleting
  the call site makes the entries unreachable, but leaving them registered
  is dead weight; remove both together in the same commit as the call site).

### Content-only replacement for the load-time toast

`confy-tui/src/tui/mod.rs:49-58` and `web/ui.ts:289-302` both currently
read `supports_comments()` to gate the one-shot `*.host.json-comments-detected`
toast. Both change their condition from `d.supports_comments()` to
`d.had_comments_at_open()` — same call shape, new method, identical
observed behavior (the toast still fires only when the file already had a
comment at open, never for a comment authored mid-session). No other line
in either function changes.

### 2. `confy-ffi` — drop the wasm binding, add the fact binding

`confy-ffi/src/lib.rs`:
- Delete `supports_comments()` (`:100-106` region).
- Add `had_comments_at_open(&self) -> bool` delegating to the new core
  trait method, same shape.

### 3. `web/confy.ts` + `web/ui.ts` + `web/types.ts` + `web/prompt.ts` + `confy-tui` — drop the callers

- `web/confy.ts::supportsComments()` — delete the wrapper; add
  `hadCommentsAtOpen()` delegating to the new ffi method.
- `web/ui.ts:298` — change `session.supportsComments()` to
  `session.hadCommentsAtOpen()` (the surrounding `if (isPlainJson && ...)`
  block is otherwise untouched — see *Content-only replacement* above).
- `web/types.ts:96-97` — delete `"JsoncUpgrade"` from the `PromptView`
  string-literal union.
- `web/prompt.ts:29-32` — delete the `JsoncUpgrade:` button-descriptor
  array entry; `:41` — delete the `JsoncUpgrade: "web.prompt.title.jsoncUpgrade"`
  title-key entry.
- `crates/confy-tui/src/tui/mod.rs:51` — change
  `d.supports_comments()` to `d.had_comments_at_open()` (same block,
  see *Content-only replacement* above).
- `crates/confy-tui/src/tui/app.rs` — delete or rewrite the two tests
  asserting on `Mode::Prompt(PromptKind::JsoncUpgrade { .. })` /
  `supports_comments()` after a remark (`app.rs:1851-1856,1870-1873`) —
  the behavior they assert (prompt appears, then flips permission) no
  longer exists; replace with a test asserting the node is remarked
  immediately with no prompt.
- `crates/confy-tui/src/lib.rs:66-75` and
  `crates/confy-core/src/model/json/doc.rs` test module (`:247-269,347-351`)
  and `model/any_doc.rs:203-206` — every existing `supports_comments()`
  test either deletes (permission-semantics tests, now meaningless) or
  is rewritten against `had_comments_at_open()` (content-detection tests:
  clean-file-false, `.jsonc`-extension handling **is no longer relevant
  input** to this fact since it was always a write-permission-only
  trigger — content detection only ever looked at the text, so the
  `jsonc_extension_supports_comments` test (`doc.rs:252-255`) has no
  equivalent and is simply deleted, not ported).
- VS Code extension (`editors/vscode/src`) — confirmed no
  `supports_comments`/`supportsComments` references exist; no change.
- Delete the i18n UI-string keys used only by the removed prompt:
  `tui.prompt.jsonc-upgrade.legend`, `web.prompt.title.jsoncUpgrade`,
  `web.prompt.btn.upgradeJsonc` (core key `core.prompt.jsonc-upgrade`
  already covered in step 1).

### 4. Domain glossary — the "JSONC upgrade" concept dissolves

`docs/reference/CONTEXT.md`'s **"JSONC upgrade"** glossary entry and
`CLAUDE.md`'s description of the same mechanism are deleted/rewritten in
the same commit as the code change (done ahead of the rest of this plan,
2026-08-28, during the grill round — see current `CONTEXT.md`/`CLAUDE.md`
for the landed text). No replacement term is needed: comments in `.json`
are unconditionally legal now, so there is no discrete "upgrade" moment
left to name. `comment_advisory` (`CONTEXT.md`'s existing entry, unchanged)
remains the term for "this row's comment is non-standard JSON."

## What stays unchanged

- `strict_json` (host-supplied "this file's real extension is plain
  `.json`" flag, `session.rs` ~L219-226 region) — untouched; still the sole
  driver of whether `comment_advisory` is computed at all.
- `comment_advisory: Option<String>` projection on `ViewRow` — untouched
  logic, now the only *authoring-time* signal.
- The one-shot `*.host.json-comments-detected` load-time toast — same
  observed behavior (fires iff the file already had a comment at open),
  now driven by `had_comments_at_open()` instead of `supports_comments()`
  (see *Content-only replacement for the load-time toast* above — this is
  a rewire, not a behavior change).
- The single JSON/JSONC lexer/parser (`model/json/parse.rs`) — was never
  gated; nothing here touches it, including the lex check that now backs
  `had_comments_at_open` (same check, new name, new home).

## Data flow (after this change)

```
.json opened, already contains a comment
  -> strict_json = true, had_comments_at_open = true (both host-driven / content-derived at open)
  -> load-time one-shot toast "json-comments-detected" (same trigger, new field name)
  -> every row rebuild: comment_advisory computed from strict_json + node facets (unchanged)

.json opened clean, user adds a comment mid-session (add_comment_sibling, remark, or split_value_comment)
  -> mutation always succeeds (no supports_comments() check, no prompt)
  -> had_comments_at_open stays false (fixed at open, this session's authored
     comment was not present when the file was opened) -- toast never
     retroactively fires
  -> next row rebuild: comment_advisory appears on the affected row
     (same code path as the "opened already containing a comment" case --
     no longer a separately-triggered promotion state)
```

## Acceptance criteria

1. `add_comment_sibling` on a clean `.json` document never returns
   `core.comment.unsupported` and never leaves `self.notice` set from this
   path.
2. `remark` on a clean `.json` document never enters
   `Mode::Prompt(PromptKind::JsoncUpgrade)`; the node is remarked
   immediately, same as TOML/YAML today.
3. `split_value_comment` splits a trailing comment off a value on a clean
   `.json` document without any prior remark/upgrade having occurred.
4. After any mutation in 1-3, the next `to_view_row` call for the affected
   row returns `comment_advisory: Some(_)`.
5. Opening a `.json` file that already contains a comment still fires the
   one-shot `*.host.json-comments-detected` toast (via
   `had_comments_at_open()`); opening a clean `.json` and authoring a
   comment mid-session never fires it.
6. `cargo build -p confy-core`, `cargo build -p confy-tui`, and the wasm
   build (`wasm-pack build --target web` for `confy-ffi`) all succeed with
   zero references to `comments_enabled`, `enable_comments`,
   `PromptKind::JsoncUpgrade`, `PendingComment`, `core.comment.unsupported`,
   or `supports_comments` remaining anywhere in the workspace (verified by
   grep, not just compiler silence — a stale i18n key or unreachable UI
   branch won't fail the build).
