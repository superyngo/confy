# JSON/JSONC parser simplification — single source of truth

Status: **consensus reached, not yet implemented.** This document is the
baseline for the follow-up session that starts fixing
`2026-08-28-comment-advisory-followup-issues.md`, beginning from the JSON
parser layer. It captures (a) how the three format parsers are actually built
today, verified against source, and (b) three architecture decisions reached
in this session, each with an explicit consensus record.

## 1. Current parser architecture (baseline facts, verified against source)

All three formats live under `crates/confy-core/src/model/` and share one
design contract: a lossless rowan CST (`GreenNode`/`SyntaxNode`) where every
byte is a token, so `parse → serialize` is byte-identical. Implementation
source differs per format:

| Format | Module | Parser implementation |
|---|---|---|
| JSON/JSONC | `model/json/parse.rs` | Hand-rolled: `lex()` tokenizer + recursive-descent `Parser`, builds the rowan tree directly. Project-owned code. |
| YAML | `model/yaml/parse.rs` | Hand-rolled: indentation-driven lexer + parser. Out-of-subset constructs (anchors/aliases/merge keys/tags/multi-line flow) fence off as `OPAQUE` nodes; multi-document (`---` more than once) rejected at load. Project-owned code. |
| TOML | `model/cst_doc.rs` | Delegates to the external `taplo` crate (`taplo::parser::parse`), itself rowan-based. The original hand-written `toml_edit`-based `TomlDocument` backend was retired after reaching parity — **no** project-owned TOML tokenizer exists anymore. |

`AnyDocument` (`model/any_doc.rs`) is the enum façade; `from_str_as` dispatches
to the matching parser by `DocFormat`. `.json` vs `.jsonc` is **not** a parser
distinction — both compile to `DocFormat::Json` and go through the same
`json/parse.rs`; the extension only matters to host-side policy (§3).

**Consequence for "starting from the most basic parser":** JSON and YAML bugs
are directly fixable in project code. A TOML tokenize/parse-stage bug can only
be worked around at the `confy-core` integration layer (`cst_doc.rs`,
`cst_edit/*`) or reported upstream to `taplo` — it cannot be patched directly.

## 2. Compile targets and per-platform overhead (baseline facts)

Only **two** binaries exist, not four:

- **Native**: `confy-tui` links `confy-core` directly (`path` dependency),
  compiled per-platform, no serialization boundary.
- **Wasm**: `confy-ffi` (`wasm-bindgen` wrapper over `confy-core`) compiles
  once via `wasm-pack build --target web` to `confy_ffi_bg.wasm` + JS glue.
  This single artifact is reused unmodified by three hosts:
  - `web/` (browser PWA) loads `web/pkg` directly.
  - `confy-tauri` has **no** `confy-core` dependency in its `Cargo.toml` at
    all — desktop/mobile is a native webview shell wrapping the same web
    bundle; all parsing happens inside the webview's wasm, not in Tauri's
    Rust layer.
  - `editors/vscode`'s webview loads `media/pkg/confy_ffi_bg.wasm`, a build-
    time copy of the same `web/pkg` artifact.

No host recompiles or reimplements a parser. The only recurring overhead is
the standard wasm-bindgen boundary cost (JS↔wasm marshaling via
`serde-wasm-bindgen` for snapshots/rows), identical across all three wasm
hosts. The one duplicated *logic* (not parsing) found is format-from-filename
string matching (`web/host-io.ts::formatFromName` vs
`editors/vscode/src/formatFromName.ts`), deliberately copy-pasted because the
VS Code extension host cannot import web internals — trivial, not a parsing
concern.

## 3. Decision 1 — `serde_json::from_str` scope

**Question:** is there still a need for raw `serde_json::from_str` anywhere
in the document-parsing path?

**Finding:** No. Both production call sites that used to call it directly —
`schema::hints::detect_hint` and `Session::parse_schema_json` — were migrated
by commit `7b20d82` to go through `AnyDocument::from_str_as` +
`schema::value_bridge::value_to_json`, specifically so a JSONC comment
anywhere in the text no longer breaks `$schema`-hint detection or schema-file
parsing.

**Remaining `serde_json::from_str` usage**, confirmed non-document-parsing:
- `session/i18n.rs` loads `i18n/en.json`/`i18n/zh-TW.json` (confy's own
  `include_str!`-embedded translation catalogs — trusted, always clean JSON,
  never JSONC, never user-supplied).
- Test-only assertions in `model/convert.rs`.
- `serde_json::Value`/`serde_json::json!` used elsewhere in `schema/*.rs` as
  the *data type* the `jsonschema` crate requires — not parsing calls.

**Decision:** No migration needed. i18n catalog loading is an intentional,
permanent exception (not a document-parsing path) and should be called out
as such wherever this is discussed again, so it doesn't get flagged as a
missed spot in a future audit.

## 4. Decision 2 — collapse JSON/JSONC to one parser + a warner

**Current state (two layers, verified):**
1. **Parser layer** — already a single parser (`json/parse.rs`). Reading a
   `//`/`/* */`/trailing-comma document has never been gated; the lexer
   always tokenizes comments regardless of extension.
2. **Write-permission gate** — `JsonDocument.comments_enabled` /
   `supports_comments()`, true iff: content already had a comment at load,
   OR host called `enable_comments()` for a `.jsonc` extension, OR the user
   accepted the interactive `JsoncUpgrade` prompt this session. This gate
   controls only whether **new** comments may be authored, via two call
   sites:
   - `add_comment_sibling` (new standalone comment node) — hard-blocked with
     the `core.comment.unsupported` Warn notice when the gate is closed.
   - `do_remark` (comment out an existing node) — routes through the
     interactive `PromptKind::JsoncUpgrade` confirm prompt
     (`PendingComment::Remark` is its only variant); accepting flips
     `comments_enabled = true` for the rest of the session.

**Consensus reached: remove layer 2 entirely, both call sites, no partial
carve-out.**

- `add_comment_sibling` and `do_remark` both stop checking
  `supports_comments()` — authoring a comment on a plain `.json` is always
  legal syntactically (single JSONC parser already accepts it on read).
- No compensating one-shot toast is added when a comment is authored mid-
  session on a previously-clean `.json`. The existing per-row
  `comment_advisory` decoration (already recomputed from `strict_json` +
  node facets on every row rebuild) is the **sole** signal for "this comment
  is non-standard JSON" — at load time and at author time alike. This
  collapses two different notification paths for the same fact into one,
  and directly removes the special-cased "promote" flow that
  followup-issue #1 suspects of skipping the advisory recompute.

**Removal scope (concrete, to inventory before implementation):**
- `JsonDocument.comments_enabled` field, `enable_comments()`,
  `supports_comments()`'s gating semantics (interface method may become
  unconditional `true`, matching TOML/YAML's existing always-`true` behavior,
  or be dropped from `ConfigDocument` entirely if nothing else reads it).
- `PromptKind::JsoncUpgrade`, `PendingComment` enum (currently one variant —
  disappears with its only caller), `PromptView::JsoncUpgrade`.
- i18n keys: `core.prompt.jsonc-upgrade`, `tui.prompt.jsonc-upgrade.legend`,
  `web.prompt.title.jsoncUpgrade`.
- `core.comment.unsupported` notice key and its `add_comment_sibling` call
  site.
- `AnyDocument::enable_comments()`, `confy-ffi`'s `supports_comments()`
  binding, `web/confy.ts::supportsComments()`, and the `.jsonc`-extension
  `enable_comments()` call in `confy-tui`'s `load_document`.

**Kept, unchanged in role:**
- `strict_json` (host-supplied "this file's real extension is plain `.json`"
  flag) stays exactly as-is — it is the trigger for `comment_advisory`, not
  part of the removed gate.
- `comment_advisory` (per-row projection) and the existing one-shot
  `json-comments-detected` load-time toast both stay, unchanged in scope —
  the toast still only fires for the "surprise" case (comments already
  present at open), never for authoring.

## 5. Decision 3 — where does a JSON warner plug into the message system

**Question:** how do parsing warnings currently reach the user, and how does
the message system handle multiple sources (schema / parsing / warner)
sharing a channel?

**Finding — three independent channels exist today, not one:**

| Channel | Type | Used by | Notes |
|---|---|---|---|
| **A. `Session.notice: Option<Notice>`** | `{severity, text, source}`, i18n-keyed, `severity_of()`-classified | `core.comment.unsupported`, `*.schema-load-error`, `*.json-comments-detected` toast | Single-slot, transient — cleared by mutation success, `Esc`, entering inline edit, and `SetLang` (documented in `docs/reference/MESSAGES.md` §1.1). |
| **B. `ViewRow.comment_advisory: Option<String>`** | Plain translated string, no severity/source | `strict_json` comment advisory (today's only user) | Persistent per-row projection, recomputed every row rebuild, entirely outside the Notice system. |
| **C. `ConvertResult.warnings: Vec<String>`** | Raw **un-translated** English strings, no severity | Cross-format convert/save-as normalization notes | Rendered as a bullet list only in the convert-confirm surface (CLI stderr, TUI `overlay_convert`, web `convert-dialog`). Bypasses i18n entirely. |

`docs/reference/MESSAGES.md` (the existing SSOT for the message system)
documents only channel A plus the Prompt-question and Diagnostic-event
channels — **it does not mention channel B or C at all.** This is a
pre-existing documentation gap, not something introduced by this plan; noted
here so it isn't mistaken for a new omission, but fixing `MESSAGES.md` itself
is out of scope for this plan.

Schema violations (`ViewRow.violations`/`has_descendant_violation`) are a
**fourth**, separate per-row projection running in parallel with B — this is
why followup-issue #2's "advisory and schema hint fight each other" is not a
single-channel collision: B and the violation projection are two independent
per-row data flows that happen to render in the same UI slots, not two writers
racing for one slot the way two `Notice` writes would.

**Consensus reached:**
- The JSON warner's output routes through **channel B only**
  (`comment_advisory`-style per-row projection). No fourth channel is
  introduced.
- Channel A keeps its current, narrow scope: the existing one-shot
  `json-comments-detected` load-time toast, unchanged. It is not extended to
  cover authoring (§4) or reused for anything new by this plan.
- Channel A's single-slot clearing/collision behavior (the mechanism behind
  followup-issue #2) is **not** addressed by this plan — it is a pre-existing
  property of the Notice system, orthogonal to the parser/warner
  simplification, and stays a separate follow-up.

## 6. Relationship to the three followup issues

| Issue | Effect of this plan |
|---|---|
| #1 (editing a comment into a clean `.json` doesn't trigger the advisory) | Directly addressed as a side effect of §4: removing the gate/prompt special-case means every comment-adding mutation path (author or remark) becomes an ordinary mutation → row rebuild → `comment_advisory` recompute, with no parallel "did the user accept the upgrade" state to fall out of sync with. Still needs a regression test confirming the recompute actually fires post-mutation. |
| #2 (advisory vs. schema hint fight on load) | Not addressed by this plan (§5). Stays a separate investigation into channel A's Notice-slot clearing order. |
| #3 (insert splits a node from its trailing comment) | Not addressed by this plan — it's a per-format CST-edit anchor-resolution bug (`json/edit.rs`, `cst_edit/move_paste.rs`, `yaml/edit/block.rs`), unrelated to the parser/gate work here. |

## 7. Explicitly out of scope for this plan

- Any change to YAML or TOML parsing, gating, or warning behavior — this plan
  is JSON/JSONC-only.
- Redesigning channel A's clearing/priority rules (issue #2).
- Unifying `ConvertResult.warnings` (channel C) into the i18n/Notice system.
- Updating `docs/reference/MESSAGES.md` to document channels B/C.

## 8. Suggested implementation order

1. Write regression tests first: (a) a clean `.json` gains a comment via
   `add_comment_sibling` and via `do_remark`, in both cases
   `comment_advisory` should appear on the next row rebuild with no prompt
   involved; (b) both actions succeed with no `core.comment.unsupported`
   notice and no `JsoncUpgrade` prompt state.
2. Remove the gate (§4's removal list) in `confy-core` first — this is the
   "most basic parser" layer the session wants to start from.
3. Remove the now-dead `enable_comments`/`supports_comments` plumbing in
   `confy-ffi`, `web/confy.ts`, and `confy-tui`'s `.jsonc`-extension handling.
4. Confirm followup-issue #1's regression test passes as a consequence, not
   as a separately-patched code path.
