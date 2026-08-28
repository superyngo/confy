# YAML quoted-key rename/edit — status memo (2026-08-28, unfinished, handed to a new session)

## User complaint (2nd round, after commit `af6adc7`)

`af6adc7` ("keep quote flanks visible while renaming a quoted YAML key") took a
**decoration-only** approach: the tree row already showed `"a b"` (via
`displayKey`/`display_key`, commit `64db70a`); on F2/rename, a static `"…"`
span was drawn around the edit `<input>`/TUI buffer, which still only held the
**decoded**, unquoted text `a b`.

User tested it and reported 3 problems, none of which happen in TOML:

1. Can't edit the quote characters themselves (they're inert decoration, not
   part of the input value).
2. A trailing space intentionally placed *inside* the quotes gets trimmed
   away on commit (`edit_commit`'s `name_str.trim()` strips the *whole*
   buffer, and the buffer never had quotes to protect the inside content).
3. The "Path" display (breadcrumb / detail popup "Path:" line — **not yet
   located/fixed**, see below) still shows the bare undecorated key, no
   quotes.

User's ask: find a more fundamental fix that doesn't regress other normal
behavior, rather than another surface patch. Manual testing after the
"fundamental" attempt below was **still not good** — user is moving to a new
session to continue. This memo is the handoff.

## Root cause (confirmed)

TOML's projected `Node.key`/`Seg::Key` **already contains the literal quote
characters** — `taplo` lexes a quoted key as an `IDENT` token whose *text*
keeps the quotes, so TOML never needed special-casing anywhere: the rename
buffer, the tree row, the Path breadcrumb, and the `Mutation::Rename`
`new_key` argument all just pass the same literal string around unchanged.

YAML's projection **decodes** the key (quotes stripped, escapes resolved)
into `Seg::Key`/`Node.key`/`ViewRow.key`, and separately tracks a coarse
`key_sign: "quoted"/"bare"/…` flag. Every consumer of the decoded key
(rename buffer, breadcrumb, etc.) never had access to the original literal
text — commit `64db70a` patched *only* the read-only tree-row label
(`displayKey`/`display_key`, a synthesized `"${key}"` wrap, not the real
source text) and `af6adc7` layered decoration on top of that for the rename
input, without touching the actual data flowing through rename/commit. That
is why it felt bolted-on and broke as soon as the user tried to actually type
in the quoted area.

## Attempted fundamental fix (uncommitted, in the working tree now)

Idea: give YAML a way to report the **literal, as-authored source text** of a
key (quotes + escapes intact, straight from the CST token) and use that
verbatim as the rename/edit buffer's content — mirroring what TOML gets for
free. Then the quote characters are just normal buffer characters (editable,
protected by the quotes from the outer `.trim()`), and `Mutation::Rename`'s
`new_key` argument naturally carries them through, symmetric with TOML.

### Changes made (compiles, confy-core lib+bin tests green, workspace test run got interrupted by the 300s job timeout partway through `serde_roundtrip.rs` — not confirmed a failure, just not observed to finish)

- `crates/confy-core/src/model/document.rs`: new `ConfigDocument::key_literal_text(&self, path) -> Option<String>` trait method, default `None`.
- `crates/confy-core/src/model/any_doc.rs`: `AnyDocument` delegates it via the existing `delegate!` macro.
- `crates/confy-core/src/model/yaml/project.rs`: new `pub(crate) fn key_literal_text(syntax, path) -> Option<String>` — walks the tree, finds the `MAP_ENTRY` at `path`, returns the raw `SINGLE`/`DOUBLE` token text verbatim (`None` for `PLAIN`/bare keys, matching TOML/JSON which return `None` from the trait default).
- `crates/confy-core/src/model/yaml/doc.rs`: `YamlDocument::key_literal_text` calls into the above.
- `crates/confy-core/src/session/inline_edit.rs`: `begin_inline_rename` and `begin_inline_edit_impl` now seed the Name-field buffer/`other_buffer`/`EditState.key` with `doc.key_literal_text(path).unwrap_or(decoded_key)` instead of always the decoded key. This also fixes a **previously-undiscovered pre-existing bug**: a Value-only edit's `frag_key` (used to rebuild the `"key: value"` fragment via `scalar_fragment`) was always the *decoded* key for YAML, so editing just the *value* of a quoted-key entry silently dropped its quotes — that's fixed as a side effect, not yet covered by a regression test.
- `crates/confy-core/src/model/yaml/edit/mutations.rs`: `rename()` reordered to parse the `new_key` probe *first*, decode it via `entry_key_name(&new_entry)`, and compare *that* (not the raw possibly-quoted `new_key`) against siblings' decoded names for the collision check — otherwise a literal `"a b"` typed by the user would never collision-match a sibling's decoded `a b`.

### NOT yet done / known-broken in the current working tree

1. **The `af6adc7` UI decoration code was never reverted.** `web/render.ts`
   (`isQuotedYamlKey` → `<span class="key-quote">"</span>` around the
   `<input>`) and `crates/confy-tui/src/tui/ui.rs` (the matching span pair
   around `edit_field_spans`) still run. Now that the buffer/input **itself**
   carries the literal quotes (core fix above), the UI layer will double them
   up: `""a b""`. **This must be reverted** — `web/kind-labels.ts`'s
   `isQuotedYamlKey` helper can stay (still used by `displayKey` for the
   read-only row), but the decoration spans in `render.ts`/`ui.rs`'s
   Name-edit branches must go, back to plain `edit_field_spans`/`<input
   value="${edit.buffer}">` with no added flanks — the buffer already *is*
   the full quoted text now.
2. **Web's `EditView`/`edit.buffer` plumbing was never re-verified** against
   this core change — need to confirm the WASM bridge round-trips
   `key_literal_text`-derived buffers correctly (should be automatic, same
   `EditState` struct serialized to JS, but not run/tested).
3. **Item 3 (Path/breadcrumb display) was never located or fixed.** Whoever
   picks this up needs to find where a "Path:" or breadcrumb string is
   rendered (TUI: likely `detail_full_text`/`ui.rs`'s detail popup;
   possibly a web equivalent in `panel.ts`) and apply the same quoting there
   — probably a `displayKey`-style read-only wrap per YAML `Seg::Key`
   segment along the path, not the literal-text plumbing (Path display is
   read-only, doesn't need to be commit-safe).
4. **No new regression tests added for this round.** The existing
   `web/render.spec.mjs` "quoted-key rename input keeps quote flanks" test
   and the TUI `quoted_yaml_key_rename_shows_quote_flanks_around_edit_buffer`
   test (both added for `af6adc7`) now assert the **decoration** behavior
   this fix obsoletes — they will need rewriting (assert the buffer/input
   value itself is `"a b"`, quotes included, not a separate flanking span)
   once decoration is reverted.
5. **`cargo test --workspace` was not run to completion** — two attempts
   both hit the 180s/300s job timeout mid-run (last seen finishing
   `confy-core`'s `schema_*` integration tests fine, then started
   `serde_roundtrip.rs` and got cut off). Needs a longer-timeout or
   `async: true` run before trusting "no regressions" for confy-tui/confy-ffi/
   the rest of confy-core's integration tests.
6. **No manual end-to-end verification** (user's own manual test was against
   the *previous* (`af6adc7`) decoration-only state, not this attempt) —
   this whole "fundamental fix" direction is unverified by a human yet.

## Suggested next steps for the new session

1. Revert the `af6adc7` decoration spans in `web/render.ts` and
   `crates/confy-tui/src/tui/ui.rs` (keep `isQuotedYamlKey`/the read-only
   `displayKey`/`display_key` wrap from `64db70a` — that part is fine and
   orthogonal).
2. Re-run/rewrite the two decoration-focused tests to instead assert the
   edit buffer/input **value** is the literal quoted text.
3. Add a regression test for the value-only-edit key-drop bug fixed as a
   side effect (§ "Changes made", `inline_edit.rs` bullet).
4. Locate and fix the Path/breadcrumb display (item 3).
5. Finish a full `cargo test --workspace` (background it / raise the
   timeout) and the web spec suite before considering this done.
6. Manually test in both the TUI and web UI: F2 rename on a quoted YAML key,
   editing/removing the quote chars, an intentional trailing space inside
   quotes, committing an unchanged rename (must not fire a mutation), and a
   collision against an existing sibling (typed with and without quotes).
