# Open-findings re-verification — 2026-09-09

Status: Resolved (2026-09-09)

Re-verifies every finding still recorded as open on 2026-09-09: the 21 items of
[2026-08-29-code-audit.md](2026-08-29-code-audit.md) (`In progress`), the three known
follow-ups in [`../reference/MESSAGES.md` §8](../reference/MESSAGES.md), and the one
`Approved`-but-unshipped plan. Nothing was fixed by this pass — it is a triage record, so
the next work session starts from measured ground truth instead of a nine-day-old claim.

Method: four read-only verification passes over the code (verdict only when a symbol could be
pointed at) plus fresh `cargo bench -p confy-core --bench perf` runs at the audit's own sizes.
Citations name files and symbols, never line numbers.

## Verdict summary

| # | Finding | Audit | Now | Note |
|---|---|---|---|---|
| 1 | `Move` is quadratic | P0 | **OPEN — worse than recorded** | TOML/JSON open; YAML already 27× faster |
| 2 | Double serialize per keystroke | P0 | **OPEN — new cause** | undo half fixed; schema-hint half is new |
| 3 | Insert into an empty document | P1 | FIXED | all three backends succeed |
| 4 | JSON rename with a quoted key | P1 | FIXED | residual: `rename_key_segs` not overridden |
| 5 | Remark an array element | P1 | **OPEN** | still three formats, three outcomes |
| 6 | No table-driven parity loop | P1 | PARTIAL | two 2-format loops; no 3-format loop |
| 7 | `taplo` abandoned upstream | P1 | CLOSED as recorded risk | documented + `cargo audit` in CI |
| 8 | God objects are 80-93% tests | P2 | PARTIAL | 3 of 4 extracted; `json/edit.rs` not split |
| 9 | Presentation re-derived in TS | P2 | FIXED | `badge_label`/`badge_note` on `ViewRow` |
| 10 | `Intent` isn't exhaustive | P3 | OPEN | ~15-20 bypass sites |
| 11 | Undo stores `String` snapshots | P3 | OPEN | `MAX_HISTORY = 200`, still text |
| 12 | `to_view_row` allocation nits | P3 | FIXED | `&'static str` → `Cow` |
| 13 | `anyhow` in public signatures | P3 | PARTIAL | `ParseError` landed; `MutateError` unchanged |
| 14 | CI gap (web specs, wasm smoke) | P3 | FIXED | web CI runs both; `cargo audit` job exists |
| 15 | No round-trip property tests | P3 | FIXED | `roundtrip_proptest.rs`, 3 backends |
| 16 | Dependency upgrades | P3 | PARTIAL | 5 done; `jsonschema`/`fuzzy-matcher` open |
| 17 | `CHANGELOG.md` too large | P3 | **OPEN — grew 42%** | 482 KB / 4,130 lines |
| 18 | `target/` is 71 GB | P1 late | OPEN | maintainer action, not a code fix |
| 19 | TUI `~` shows the oldest 20 | §8 | OPEN | newly recorded 2026-09-09 |
| 20 | Web `lastSeenSeq` not reset | §8 | OPEN | 8 unreset session-replacement paths |
| 21 | Touch `sev-*` toasts have no CSS | §8 | OPEN | cosmetic, MVP-deferred |
| 22 | JSON/JSONC parser SSOT plan | plan | **FIXED — premise refuted** | plan can be closed |

Net: **8 FIXED, 5 PARTIAL, 9 OPEN**. Two of the audit's headline P0s are still open, and both
are *differently* open than recorded.

---

## 1. `Move` is quadratic — OPEN, and worse than the audit measured

Re-measured today on this machine. The audit's numbers were not reproducible as *upper*
bounds; every one is exceeded:

| projected nodes | `apply(Replace)` | `apply(Move ×1)` | audit's `Move ×1` | `Move ×8` |
|---|---|---|---|---|
| 2,801 | 5.50 ms | 68.1 ms | 87.5 ms | 505 ms |
| 7,001 | 14.8 ms | **527 ms** | 752 ms | **4.18 s** |
| 98,001 | 542 ms | **101.2 s** | 40.2 s @ 70k | (budget blown) |

The shape is unambiguous: 14× the nodes costs 36× a `Replace` but **192×** a single-source
`Move`. At 7k nodes a one-node move is already 36 `Replace`s.

**The decisive new datum is the YAML column, which the audit never measured:**

| projected nodes | TOML `Move ×8` | YAML `Move ×8` | ratio |
|---|---|---|---|
| 2,801 | 505 ms | 30.8 ms | 16× |
| 7,001 | 4.18 s | 79.1 ms | **53×** |

YAML's `move_nodes` (`yaml/edit/mutations.rs`) already threads one `walk` result through the
opaque check, the fragment capture and the index shift — it only re-walks inside its delete
loop. TOML re-walks for setup, then once per source inside `delete` (`cst_edit/replace_delete.rs`),
then once per fragment inside the re-insert loop: `1 + |S| + |F|` full walks. JSON is
`2·|S| + 1` (each `serialize_fragment` → `resolve` → `walk`, plus a whole `project()` for the
shift, plus a `walk` per `delete`).

So the fix does not need designing — **YAML is the working reference implementation**, and its
53× margin is the measured prize. This also downgrades the audit's "one shared fix for three
backends": YAML is largely done; TOML and JSON need the same treatment, with TOML's delete loop
the single biggest term.

Estimate stands at **S effort / Low risk**, now with an empirical target.

## 2. Double serialize per keystroke — OPEN, but the cause has moved

The audit's exact claim is **fixed**: `ConfigDocument::apply` returns `Result<String, _>`, all
three backends `Ok(text)`, and `Session::on_mutation_success(touched, text)` pushes that very
string into `History` — the undo snapshot no longer serializes.

A second full serialize survives on the same path, via a different route:

```
doc.apply(m) -> text -> on_mutation_success(touched, text)
                          -> history.push(text)          0 serialize
                          -> revalidate_schema()
                          -> sync_schema_hint()
                               -> detect_and_request_schema()
                                    -> doc.serialize()    1 serialize   <-- here
```

`Session::detect_and_request_schema` re-serializes the whole document *only* to hand the text
to `schema::hints::detect_hint`. It runs on every mutation regardless of whether the document
has a schema hint at all. At 98k nodes that is a **16.4 ms** serialize per keystroke (16.4 ms
measured as `serialize()` in the same run); at 7k nodes ~1.2 ms, matching the audit's figure.

Fix is smaller than the original one: `on_mutation_success` already holds the text — pass it
into `sync_schema_hint`/`detect_and_request_schema` instead of re-deriving it. The other two
`sync_schema_hint()` callers are in `undo_redo.rs`, where the text is also already in hand
(it is what was just restored). **XS effort / Low risk.**

## 3-6. Cross-backend drift — two of three divergences fixed

- **Empty-document insert: FIXED.** All three now branch on `target.parent.is_empty()` —
  TOML in `cst_edit/tree_nav.rs`'s `resolve_insert_at`, and JSON/YAML through matching
  `insert_into_empty_document` functions reached from an `Err(NotFound) if parent.is_empty()`
  arm. JSON's is covered by `insert_member_into_empty_document`.
- **JSON quoted-key rename: FIXED.** `json/edit.rs`'s `rename` now detects an
  already-quoted input (`new_key.starts_with('"') && new_key.ends_with('"')`) instead of
  blind-wrapping, and its collision check compares `key_name_of(&sib_key_node)` against a
  decoded `decoded_new_key` — literal-vs-decoded is gone. *Residual:* `JsonDocument` still
  uses `ConfigDocument`'s default `rename_key_segs`, where TOML and YAML both override it to
  decode with their own key lexer. Not a live defect (JSON keys are always quoted, so the
  default's behavior coincides), but it is the one asymmetry left in this area.
- **Remark an array element: OPEN.** Unchanged, and still the cleanest illustration of the
  problem — one gesture, three outcomes:

  | backend | outcome |
  |---|---|
  | TOML | `Err(Unsupported)` — falls into `remark`'s wildcard arm |
  | JSON | `Err(Illegal("cannot remark an array element"))` |
  | YAML | `Ok(())` — succeeds |

  Two of the three are *also* inconsistent about which error variant means "this gesture does
  not apply here", which is a message-severity difference the user sees.
- **Parity test gap: PARTIAL.** Two files now iterate formats
  (`external_edit_clears_trailing_comment.rs` over Json+Toml, `insert_after_trailing_comment.rs`
  over Toml+Yaml) — but each leaves the third format in a separate block below the loop, which
  is exactly the shape that lets drift through. `hostile_input.rs` iterates all three, but for
  recursion-depth fuzzing, not semantics. `session_headless.rs` has zero format loops.
  A table-driven `Remark`-an-array-element case would fail today on all three backends at once.

## 7-9. Risk recorded, tests extracted, TS drift closed

- **taplo:** recorded as a known risk in `CLAUDE.md` with the 1,240-LOC vendoring scope, and
  `rust-ci.yml` has an active `cargo audit` step — the trigger the audit asked for. Treating
  this as closed *as a decision*; the dependency is of course still unmaintained.
  (One correction landed 2026-09-09: taplo's DOM **is** used, for the TOML duplicate-key
  backstop. Vendoring scope is unchanged — that call becomes the hand-rolled check the JSON and
  YAML backends already have.)
- **God objects:** three of four extracted to sibling `tests.rs` via `#[path = "tests.rs"]`:
  `cst_edit/mod.rs` 295 production lines (tests 3,191), `tui/app.rs` 1,009 (tests 3,553),
  `yaml/edit/mod.rs` 147 (tests 2,206). Remaining: `model/json/edit.rs`, 2,864 lines with
  1,090 of inline tests and no `block`/`flow`/`mutations`/`convert` split — the one genuinely
  monolithic production file, exactly as the audit said.
- **TS presentation drift:** `ViewRow` now carries `badge_label`/`badge_note` as
  `Cow<'static, str>`, `web/kind-labels.ts` no longer re-derives badges, and
  `web/help-content.ts` reads `t("web.help.legend.…")` instead of hardcoded constants.

## 10-18. Smaller items

**Open.** `Intent` bypass (~15-20 direct `session.<field> =` / non-dispatch calls in
`tui/app.rs` and `tui/mod.rs` — e.g. `self.session.mode = Mode::Normal`, `session.paste_slot =
Some(…)` — each one invisible to the diag ring and to record/replay). `History` still stores
`String`, `MAX_HISTORY = 200`. `CHANGELOG.md` has **grown 42%** since the audit flagged it
(482.7 KB / 4,130 lines vs 341 KB / 2,042) — the trend is the finding, not the size.
`target/debug/incremental` still exists and there is no `.cargo/config.toml` or
`CARGO_INCREMENTAL` setting anywhere; this one is maintainer action, not a code change.

**Fixed.** `to_view_row` returns `&'static str` into `Cow`. `web-ci.yml` runs
`functional_smoke.mjs` **and** `npm test`. `proptest = "1"` is a `confy-core` dev-dependency
with `{toml,json,yaml}_fixture_roundtrips` in `roundtrip_proptest.rs`.

**Partial.** `AnyDocument::from_str_as` returns `Result<Self, ParseError>` — `anyhow` is out
of the parse signature — but `MutateError` still mixes interactive outcomes (`Collision`,
`Fragment`) with real errors (`NotFound`, `Illegal`, `Unsupported`), so a host cannot tell
"the user needs to answer a prompt" from "this is a bug" by type alone. Deps: `thiserror 2`,
`unicode-width 0.2`, `dirs 6`, `ratatui 0.30`, `crossterm 0.29` all landed; `jsonschema` is
still `0.30` (0.44 available), `fuzzy-matcher 0.3` still unmaintained, `ureq 2` deliberately
deferred.

## 19-21. The `MESSAGES.md` §8 follow-ups

- **TUI `~` overlay shows the oldest 20 events — OPEN.** Confirmed unchanged:
  `draw_diag_overlay` does `app.session.diag.iter().collect()` (oldest → newest), sizes the box
  `lines.len().min(20)`, and hands the full `Vec` to a `Paragraph`, which renders from line 0.
  No scroll state, no `.rev()`, no tail-take; `App` has no diag scroll field. Ring
  `CAPACITY = 256`. Exactly three `diag.push` sites: `dispatch` (Debug) and `mutation`
  (Error-or-Info) in `session/dispatch.rs`, `notice` (Info) in `session/session.rs`. So a
  dispatch that sets a notice costs 3 events and one that doesn't costs 2 — the overlay goes
  blind after **7 interactions with notices, 10 without**. No other host renders the ring, so
  the bug is TUI-only.
- **Web `lastSeenSeq` not reset — OPEN, and broader than recorded.** `web/ui.ts`'s
  module-level `lastSeenSeq = -1` is advanced only inside `drainDiagIfEnabled` and reset
  **nowhere**; there are **8** paths that replace the `ConfySession` (whose ring restarts at
  `seq = 0`). Touch has no drain at all, so the `?diag=1` trace is desktop-only. Still
  debug-only blast radius; still a one-line fix, but it belongs next to a single
  session-replacement helper rather than at each of the 8 sites.
- **Touch `sev-*` toasts have no CSS — OPEN.** `web/touch/app.ts`'s `renderNotice` applies the
  classes; `web/touch/style.css` has `.toast`/`.toast.show` and **zero** `sev-*` rules, so a
  `Warn` differs from a `Success` only by its auto-hide timer (3000 ms vs 1600 ms). Note
  `web/style.css` *does* style `sev-warn`/`sev-success` — for the desktop footer status line,
  not the toast — so the two hosts disagree on whether severity is visible at all.

## 22. The JSON/JSONC parser-simplification plan — premise refuted, close it

`docs/plan/2026-08-28-json-jsonc-parser-simplification-ssot.md` is `Approved`. Verification
says **both** halves are effectively done: the comment write-gate is gone (`JsonDocument`
exposes `had_comments_at_open` instead of a `comments_enabled` gate, and comments are legal in
every `.json` document), and the "unify the two parsers" half has no work left because there is
only one parser — `model/json/parse.rs` — and always was at the code level. The plan should be
re-read and marked `Shipped`/`Superseded` rather than left `Approved`, which currently reads as
"agreed work not yet started".

---

## Suggested sequence (revised)

The audit's original ordering still holds, with two amendments: the serialize fix is now
XS-effort and independent, and `Move` has a working in-repo reference.

1. **Thread `text` into `sync_schema_hint`** (#2). XS, isolated, removes a full serialize per
   keystroke. Do this first — it is the cheapest measurable win in the list.
2. **De-quadratic TOML then JSON `move_nodes`** (#1), modelled on YAML's index threading.
   Verify with `cargo bench -p confy-core --bench perf -- --nodes 500`, target: TOML `Move ×8`
   from 4.18 s toward YAML's 79 ms.
3. **One table-driven parity loop over all three `DocFormat`s** (#6), seeded with
   Remark-an-array-element, then pick the single correct outcome and make all three agree (#5).
   Decide the semantics before the code: `Unsupported` vs `Illegal` is a user-visible severity
   difference.
4. **TUI diag overlay tail-take** (#19) and **web `lastSeenSeq` reset** (#20) — both tiny, both
   clear a recorded follow-up.
5. Split `model/json/edit.rs` (#8) and extract its inline tests.
6. Close out the JSON plan's status (#22); split `CHANGELOG.md` (#17).

Deferred with reason: `Intent` exhaustiveness (#10) and `GreenNode` undo (#11) are real but
M-effort with no user-visible symptom today; `MutateError` restructuring (#13) is best done
alongside #3, since that is what exposes the variant confusion.
