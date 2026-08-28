# Plan — make a key's *authored spelling* a first-class projection output

✅ **Shipped — historical reference.** See `CHANGELOG.md` for current behavior; this plan is kept
for context, not as a live task list. Landed 2026-08-28 (CHANGELOG entries
`2026-08-28T14:30:00Z` and `2026-08-28T18:00:00Z`).
**Supersedes the patch line:** `64db70a` → `af6adc7` → `4795e89` → `8ef6af0`
(`../debug/2026-08-28-yaml-quoted-key-edit-memo.md`).

## Problem statement

`Node` carries a key's spelling in a **lossy** form and each consumer that needs
the real spelling either re-walks the CST or *invents* one:

- `key_sign: KeySign` is a 4-value enum (`Bare|Quoted|Dotted|None`). It records
  *that* a key was quoted, never *how*.
- `ConfigDocument::key_literal_text(path)` (YAML-only) is a second, divergent
  KEY-token lookup that re-`walk()`s the whole tree per call
  (`yaml/project.rs:863-878`, O(n)).
- Three separate call sites synthesize a quote character from `key_sign`, all
  hardcoding `"`: `confy-tui/src/tui/ui.rs:66-68`,
  `confy-core/src/session/session.rs:247-250`, `web/kind-labels.ts:136-142`.

Meanwhile the three backends disagree on what `Node.key`/`Seg::Key` even *is*:

| backend | `Seg::Key` content | literal available? |
|---|---|---|
| TOML | **raw source text, quotes included** (taplo lexes a quoted key as `IDENT` keeping quotes) | implicitly, it *is* the key |
| YAML | **decoded** (quotes stripped, some escapes resolved) | only via `key_literal_text` |
| JSON | **decoded** | no |

TOML's "it just works" is an accident of taplo's lexing, not a design — and
`cst_project.rs:437-439` contains an `unquote()` branch for keys that is **dead
code**, i.e. the original author intended the opposite.

## Verified evidence

All reproduced on the real code via `crates/confy-core/tests/scratch_key_repr_repro.rs`
(temporary harness, delete when this lands).

1. **Single-quoted YAML key renders with double quotes.**
   `'a b': 1` → Detail "Path:" line = `"a b".c`; `ViewRow.path_display` = `"\"a b\""`.
   Cause: hardcoded `'"'` at `session.rs:248,250`. Same in the tree row via
   `ui.rs::display_key` and `web/kind-labels.ts::displayKey`.
2. **TOML→JSON corrupts a quoted key.** `"a b" = 1` converts to
   `{ "\"a b\"": 1 }` — the quotes are baked into the JSON key.
   Cause: `convert.rs:258-261` uses `Seg::Key(k)`, which for TOML is the raw text.
3. **TOML's `Seg::Key` is `Key("\"a b\"")`.** Confirmed by projection dump.
   The same mechanism means `schema/hints_edit.rs:78-81` and
   `schema/dirty_check.rs:134` look up JSON-Schema `properties["\"a b\""]` and
   always miss → a quoted TOML key silently gets no schema hints/validation.
   *(Inferred from the shared mechanism; not separately repro'd.)*
4. **YAML's key decoder is incomplete and divergent from the YAML spec.**
   `"a\tb"` → decoded key contains a **raw tab** (so the tree row prints a real
   tab); `"a\x20b"` and `"a\u0020b"` are **not** decoded at all (the backslash
   sequence survives into `Seg::Key`).
5. **(4) defeats the rename collision check and emits an invalid document.**
   In YAML `"a\x20b"` and `a b` are the same key. Renaming a sibling to `a b` in
   a file already holding `"a\x20b"` is accepted, producing:
   ```yaml
   "a\x20b": 1
   a b: 9
   ```
   — two YAML-identical keys, silently written.
6. **Not reproduced:** the reported "external editor 資料不見". Every
   core-level external-edit path round-trips correctly (unchanged commit, value
   change, container edit, in-editor rename, editor-restyled quoting, both quote
   styles). **Needs the user's exact repro steps**; the suspect is the host
   layer (TUI `$EDITOR` temp-file flow in `confy-tui`, or the web `#ext-modal`),
   not `Session::apply_external_replace`.

## Design

`Node` gains one field, filled **once** during projection, from the token that
is already in hand:

```rust
/// The key exactly as authored in the source — quotes, escapes and all.
/// `None` for keyless nodes (array elements, AoT entries, Root, comments),
/// i.e. the same nodes where `key_text_range` is `None`.
pub key_literal: Option<String>,
```

Invariants, enforced for **all three** backends:

- `Node.key` / `Seg::Key` = the **decoded** key. Semantic identity. Used by
  path resolution, collision checks, schema lookup, `to_value`/convert, serde.
- `Node.key_literal` = the **source text**. Presentation + edit identity. Used
  by the tree row, the Path line, the rename/edit buffer, fragment rebuilding.
- `KeySign` is no longer stored; it is *derived* from `key_literal` for the
  type-filter facet and the Detail "Sign" label.

This is a strict information gain: one field replaces `key_sign` +
`key_literal_text()` + three quote-synthesizing helpers.

## Tasks

### Phase 1 — Model
1. `model/node.rs`: add `Node.key_literal: Option<String>`; keep `KeySign` the
   *type* but derive it (`fn key_sign(&self) -> KeySign` reading `key_literal`
   + `Node.path.len()` for `Dotted`); remove the `key_sign` field.
   → verify: `cargo build -p confy-core` compiles after the mechanical fixups.
2. `model/document.rs` + `model/any_doc.rs`: **delete**
   `ConfigDocument::key_literal_text` and its `AnyDocument` delegation.
   `model/yaml/project.rs`: delete `pub(crate) fn key_literal_text`.
   → verify: no references remain (`grep key_literal_text` = 0 hits).

### Phase 2 — Producers (one per backend, all three must agree)
3. **YAML** `yaml/project.rs`: `key_name_and_sign` → `key_name_and_literal`
   returning `(decoded, Option<String literal>)` from the same token it already
   inspects. Thread `key_literal` through the 13 `Node` literal sites.
   → verify: new projection unit tests for `a`, `"a b"`, `'a b'`, `'it''s'`,
   `"a\tb"`, `"a\x20b"`.
4. **YAML decoder completeness**: extend `yaml::edit::decode_double` to handle
   `\xNN` and `\uNNNN` (currently unhandled) so the decoded identity matches the
   YAML spec — this is what closes evidence item 5.
   → verify: a test asserting `"a\x20b"` and `a b` are the *same* `Seg::Key`,
   and that renaming into that clash is rejected as a collision.
5. **TOML** `cst_project.rs`: `key_segments` must return the **decoded** key
   (activate the currently-dead `unquote` branch and apply it to the
   quote-carrying `IDENT` text too); add a parallel
   `key_literals()` returning the raw token text per segment; thread
   `key_literal` through the 13 `Node` literal sites.
   → verify: projection test that `"a b" = 1` yields `Seg::Key("a b")` +
   `key_literal == Some("\"a b\"")`; TOML→JSON of `"a b" = 1` emits
   `{ "a b": 1 }`; a schema-hint test for a quoted TOML key now resolves.
   ⚠ **Blast radius**: this changes TOML path identity. Audit every
   `Mutation` path construction, clipboard/undo `History` entry, and
   `cst_edit/rename.rs:26` / `move_paste.rs:1069-1122` for assumptions that a
   path segment is re-emittable as source text.
6. **JSON** `json/project.rs`: set `key_literal = Some("\"k\"")` from the member's
   `STRING` token (12 sites).
   → verify: projection test.

### Phase 3 — Consumers (each one deletes a patch)
7. `session/session.rs::human_path`: replace the hardcoded `'"'` wrap with
   `node.key_literal` when present, else the decoded key. Drives both the
   Detail "Path:" line and `ViewRow.path_display`.
   → verify: `'a b'` shows `'a b'`; `"a b"` shows `"a b"`; TOML shows `"a b"`
   once, not twice; bare keys unchanged.
8. `ViewRow`: add `key_literal: Option<String>` (additive-safe — census
   confirmed no `serde(deny_unknown_fields)`, single construction site at
   `session.rs:186`); keep `key_sign: String` as the derived label for the
   Detail/panel "Sign" field.
   → verify: `tests/serde_roundtrip.rs` updated and green.
9. `confy-tui/src/tui/ui.rs`: `display_key` becomes
   `row.key_literal.unwrap_or(row.key)` — **delete** `is_quoted_yaml_key` and
   the per-format special cases (`DocFormat::Toml` no-double-wrap,
   `Json` no-wrap). Same for `web/kind-labels.ts` (`displayKey`,
   `isQuotedYamlKey`) and its two call sites (`web/render.ts:173`,
   `web/touch/render.ts:106`).
   → verify: rewrite `web/render.spec.mjs:127-201` and the TUI
   `display_key_*` tests against `key_literal`.
10. `session/inline_edit.rs:52-56,143-147`: seed the rename/edit buffer and
    `frag_key` from `row.key_literal` instead of calling `doc.key_literal_text`.
    → verify: existing quoted-key rename/value-edit tests still green; add a
    single-quoted-key rename no-op test (must not restyle to double quotes).
11. `session/type_filter.rs` + `status_fmt.rs::key_sign_label`: take the derived
    `KeySign` from `Node::key_sign()`.
    → verify: existing `type_filter` tests green.

### Phase 4 — Verification
12. Full `cargo test --workspace` (background it; prior sessions hit the 300s
    timeout mid-run), `node --test web/*.spec.mjs`, `tsc --noEmit`.
13. Delete `crates/confy-core/tests/scratch_key_repr_repro.rs`; its cases live
    on as real tests in `session_headless.rs` / the projection modules.
14. Manual pass in TUI **and** web: single- and double-quoted YAML key —
    tree row, Path line, F2 rename, quote-char editing, inside-quote trailing
    space, no-op commit, collision typed both ways; quoted TOML key — tree row,
    Path line, rename, TOML→JSON convert, schema hints.
15. `CHANGELOG.md` "Unreleased Update" entry + update
    `docs/reference/yaml-quoted-key-edit-memo.md` (mark resolved, point here),
    `docs/reference/TUI.md`, `docs/reference/WEBUI.md` (`key_sign` →
    `key_literal`), `CONTEXT.md` if it documents `Node`'s fields. One commit.

## Out of scope
- The unreproduced external-editor data loss (evidence item 6) — separate
  investigation once repro steps arrive. It may be fixed incidentally by task 10.
- `Seg::Key(KeyRepr { decoded, literal })` (the "Option B" of the design
  discussion): purer, but it changes the path wire format across FFI/web,
  clipboard and undo history. Rejected as not worth the blast radius.

## Risks
- **Task 5 is the risky one.** Changing TOML path identity from raw to decoded
  touches mutation/clipboard/undo assumptions. If the audit turns up deep
  coupling, land Phases 1–4 for YAML/JSON first and split TOML into its own
  commit behind its own test pass.
- `key_sign` becoming derived changes the Detail "Sign" label for TOML dotted
  keys if `Dotted` detection is not reproduced faithfully — pin it with a test
  before deleting the stored field.
