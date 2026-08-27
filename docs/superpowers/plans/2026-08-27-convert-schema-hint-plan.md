✅ **Shipped — historical reference.** See `CHANGELOG.md` for current behavior; this plan is kept for context, not as a live task list.

# Carry the schema hint's writing convention across convert/save-as

## Context

`confy` detects a document's `$schema` hint using three separate per-format
conventions (`crates/confy-core/src/schema/hints.rs`): a JSON root `"$schema"`
string field, a YAML leading `# yaml-language-server: $schema=<path>`
modeline, and a TOML first-line `#:schema <path>` comment. The document-level
converter (`crates/confy-core/src/model/convert.rs`, `convert()`, spec
§Phase 4) currently has zero awareness of any of these conventions — verified
by reading the full `convert.rs`, its `#[cfg(test)] mod tests`, and
`confy-tui/tests/convert_cli.rs`: no reference to `schema`, `$schema`,
`#:schema`, or `yaml-language-server` anywhere in the conversion path. It
lowers the source to a format-neutral `Value` tree and renders the target's
default style, so a JSON `$schema` field survives as an ordinary **data key**
in a YAML/TOML target (polluting the output, unrecognized by
`hints::detect_hint` on re-open), and a YAML/TOML hint comment survives as an
inert comment using the *source* format's wording (unrecognized by
`detect_hint` on the target format — the hint is silently lost).

This plan makes `convert()` detect a source hint up front, strip its
source-format marker from the neutral tree, and — when representable —
re-inject it into the target format's own convention, warning (via the
existing lossy-conversion warning mechanism) when the target's root shape
can't carry a hint at all. It also formalizes "Schema hint" as its own
`docs/reference/CONTEXT.md` glossary term (currently undocumented there) and
centralizes per-line hint recognition in `schema/hints.rs` so detection and
conversion can never drift apart on what counts as a hint line.

## Approach

1. **Centralize per-line hint recognition in `crates/confy-core/src/schema/hints.rs`**,
   so `model/convert.rs`'s strip/inject logic (steps 3–4) and `detect_yaml`/
   `detect_toml` share one predicate each, instead of duplicating the
   recognition rule in two modules. Extract the inner checks — the part that
   runs on a line's text *after* its `#` comment marker is already stripped —
   into two new `pub(crate)` functions, and rewrite `detect_yaml`/`detect_toml`
   to call them (`detect_json` is unchanged; JSON's `$schema` has no
   per-line/marker concept). Behavior of `detect_hint` must stay
   byte-for-byte identical — the existing tests in
   `crates/confy-core/tests/schema_headless.rs` (`detect_hint_yaml_modeline`,
   `detect_hint_yaml_none_when_modeline_not_leading`,
   `detect_hint_toml_first_line_schema_comment`,
   `detect_hint_toml_none_when_not_first_line`,
   `detect_hint_toml_none_when_no_separator_after_schema`,
   `detect_hint_yaml_none_when_modeline_schema_value_empty`, and the JSON
   `detect_hint_json_*` tests) are the regression guard; all must keep
   passing unmodified.

   Replace the current `detect_yaml` (`hints.rs:45-64`) with:
   ```rust
   fn detect_yaml(text: &str) -> Option<SchemaSource> {
       // "Leading" = the modeline must appear before any non-comment,
       // non-blank line (a real document line breaks the leading-comment run).
       for line in text.lines() {
           let trimmed = line.trim_start();
           if trimmed.is_empty() {
               continue;
           }
           if let Some(rest) = trimmed.strip_prefix('#') {
               if let Some(schema) = yaml_modeline_value(rest) {
                   return to_source(schema.trim());
               }
               continue; // some other leading comment — keep scanning
           }
           return None; // first non-comment, non-blank line — stop
       }
       None
   }

   /// The `$schema` path/URL from a YAML modeline's text *after* the `#`
   /// marker (leading/internal whitespace tolerated), or `None` if
   /// `after_hash` isn't a `yaml-language-server: $schema=...` line. Shared
   /// by `detect_yaml` (raw source, `#` stripped per line) and
   /// `model::convert`'s hint strip/inject (already comment-marker-stripped
   /// `Item::Comment` text) so both recognize exactly the same line.
   pub(crate) fn yaml_modeline_value(after_hash: &str) -> Option<&str> {
       after_hash
           .trim_start()
           .strip_prefix("yaml-language-server:")?
           .trim_start()
           .strip_prefix("$schema=")
   }
   ```
   Replace the current `detect_toml` (`hints.rs:66-73`) with:
   ```rust
   fn detect_toml(text: &str) -> Option<SchemaSource> {
       let first_line = text.lines().next()?;
       let rest = first_line.strip_prefix('#')?;
       to_source(toml_hint_value(rest)?)
   }

   /// The `:schema` path/URL from a TOML first-line hint's text *after* the
   /// `#` marker, or `None` if `after_hash` isn't a `:schema <path>` line.
   /// Shared the same way as `yaml_modeline_value`.
   pub(crate) fn toml_hint_value(after_hash: &str) -> Option<&str> {
       let rest = after_hash.strip_prefix(":schema")?;
       if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
           return None;
       }
       Some(rest)
   }
   ```
   (`first_line.strip_prefix('#')` then `toml_hint_value`'s
   `strip_prefix(":schema")` is byte-for-byte equivalent to the original
   single `first_line.strip_prefix("#:schema")` — no char can sit between
   `#` and `:schema` in either form.)

2. **Add the import** to `crates/confy-core/src/model/convert.rs`, extending
   the existing `use` block (currently lines 17–20) with
   `use crate::schema::{hints, SchemaSource};`.

3. **Detect the source hint, strip its source-format marker, re-inject in
   the target's convention.** Replace `convert()` (currently lines 34–49)
   with:
   ```rust
   pub fn convert(doc: &AnyDocument, target: DocFormat) -> Result<ConvertResult, ConvertAbort> {
       let hint = hints::detect_hint(&doc.serialize(), doc.format());
       let (mut value, mut warnings) = doc.to_value()?;
       if hint.is_some() {
           strip_hint_item(&mut value, doc.format());
       }
       analyze(&value, target, &mut warnings)?;
       if let Some(src) = &hint {
           inject_hint(&mut value, target, hint_raw(src), &mut warnings);
       }

       let text = match target {
           DocFormat::Toml => render_toml(&value)?,
           DocFormat::Json => render_json(&value),
           DocFormat::Yaml => render_yaml(&value),
       };

       reparse_check(&text, target)?;

       warnings.sort();
       warnings.dedup();
       Ok(ConvertResult { text, warnings })
   }
   ```
   (`analyze`, `render_toml`/`render_json`/`render_yaml`, `reparse_check` are
   the existing functions, unchanged, already taking `&Value`/`&mut
   Vec<String>` by reference.) When `hint` is `None` (no hint detected, e.g.
   an empty `"$schema": ""` — `detect_hint` already treats that as "not a
   valid hint"), steps 3's new branches are skipped entirely and behavior is
   byte-for-byte unchanged from today.

4. **Add six new private helpers** in `convert.rs`, placed after `convert()`
   and before the existing `// ── source → Value ──` section comment:
   ```rust
   fn hint_raw(source: &SchemaSource) -> &str {
       match source {
           SchemaSource::Local(s) | SchemaSource::Url(s) => s,
       }
   }

   fn strip_hint_item(value: &mut Value, src_format: DocFormat) {
       match src_format {
           DocFormat::Json => {
               if let Value::Map(items) = value {
                   if let Some(idx) = items.iter().position(|it| {
                       matches!(it, Item::Node { key: Some(k), .. } if k == "$schema")
                   }) {
                       items.remove(idx);
                   }
               }
           }
           DocFormat::Yaml => {
               strip_leading_comment_line(value, |line| hints::yaml_modeline_value(line).is_some())
           }
           DocFormat::Toml => {
               strip_first_line_comment(value, |line| hints::toml_hint_value(line).is_some())
           }
       }
   }

   /// YAML: the modeline may be any physical line within the leading run of
   /// standalone comments (mirrors `hints::detect_yaml`'s scan). Removes
   /// just that line, splitting its parent merged comment block when it
   /// shares one with unrelated text (consecutive `#` lines with no blank
   /// line between them merge into one `Item::Comment` —
   /// `model/yaml/project.rs`'s `comments_standalone_trailing_merged` test);
   /// drops the whole `Item` only if the block becomes empty. Stops at the
   /// first non-`Comment` item, matching `detect_yaml`'s "first
   /// non-comment, non-blank line ends the run".
   fn strip_leading_comment_line(value: &mut Value, is_hint_line: impl Fn(&str) -> bool) {
       let items = match value {
           Value::Map(items) | Value::Seq(items) => items,
           _ => return,
       };
       for item in items.iter_mut() {
           let Item::Comment(text) = item else { break };
           if let Some(pos) = text.lines().position(|l| is_hint_line(l)) {
               remove_line(text, pos);
               break;
           }
       }
       items.retain(|it| !matches!(it, Item::Comment(t) if t.is_empty()));
   }

   /// TOML: the hint must be the file's literal first line (mirrors
   /// `hints::detect_toml`), so only the root's first item's first line is
   /// ever a candidate — no scan needed.
   fn strip_first_line_comment(value: &mut Value, is_hint_line: impl Fn(&str) -> bool) {
       let items = match value {
           Value::Map(items) => items,
           _ => return,
       };
       if let Some(Item::Comment(text)) = items.first_mut() {
           if text.lines().next().is_some_and(&is_hint_line) {
               remove_line(text, 0);
           }
       }
       items.retain(|it| !matches!(it, Item::Comment(t) if t.is_empty()));
   }

   /// Remove physical line `idx` from a (possibly multi-line, `\n`-joined)
   /// comment block's text in place.
   fn remove_line(text: &mut String, idx: usize) {
       *text = text
           .lines()
           .enumerate()
           .filter(|(i, _)| *i != idx)
           .map(|(_, l)| l)
           .collect::<Vec<_>>()
           .join("\n");
   }

   fn inject_hint(value: &mut Value, target: DocFormat, raw: &str, warnings: &mut Vec<String>) {
       match target {
           DocFormat::Json => match value {
               Value::Map(items) => items.insert(
                   0,
                   Item::Node {
                       key: Some("$schema".into()),
                       value: Value::Str(raw.to_string()),
                       trailing: None,
                   },
               ),
               _ => warnings.push(
                   "schema hint dropped: JSON $schema requires an object root".into(),
               ),
           },
           DocFormat::Yaml => match value {
               Value::Map(items) | Value::Seq(items) => items.insert(
                   0,
                   Item::Comment(format!("yaml-language-server: $schema={raw}")),
               ),
               _ => warnings.push(
                   "schema hint dropped: YAML modeline requires a mapping or sequence root".into(),
               ),
           },
           DocFormat::Toml => match value {
               Value::Map(items) => items.insert(0, Item::Comment(format!(":schema {raw}"))),
               _ => warnings.push("schema hint dropped: TOML root must be a table".into()),
           },
       }
   }
   ```
   `render_toml_table`'s existing Phase A loop prints `Item::Comment` entries
   in declaration order before any sub-table headers, so an `Item::Comment`
   at index 0 of the root `Value::Map`'s items renders as the file's literal
   first line — satisfying `detect_toml`'s "must be the first line" rule.
   `render_yaml_map`/`render_yaml_seq` render `Item::Comment` at index 0 as
   the first `#` line; `render_json_value` renders `Item::Node` at index 0
   as the first object member (`"$schema": "..."`) — both existing,
   unchanged. Every helper takes `&mut Value`/mutates in place, matching the
   ownership already established by `convert()`'s `let (mut value, ...)`.

5. **Formalize "Schema hint" in `docs/reference/CONTEXT.md`.** Add a new
   entry to the `### Schema` section (currently lines 291–315), placed
   directly after the section's opening `Validation runs on the
   \`jsonschema\` crate...` line (currently line 293) and before the
   existing **JSON projection** entry (currently line 295):
   ```markdown
   **Schema hint**:
   The in-document pointer to a JSON Schema, recognized per-`DocFormat` by
   `schema::hints::detect_hint` — a JSON root `"$schema"` string member, a
   YAML leading `# yaml-language-server: $schema=<path>` modeline, or a
   TOML first-line `#:schema <path>` comment. Distinct from **Comment**:
   even though the YAML/TOML forms are lexically comments, a Schema hint is
   recognized, stripped, and re-authored in the target's own convention
   during **Conversion** — the "comments carry across" rule never governed
   it. Distinct from **SchemaSource**: SchemaSource is the *parsed* pointer
   (`Local(path)` / `Url(url)`); Schema hint is the in-document *marker*
   that encodes one.
   _Avoid_: Comment (a Schema hint's marker form is comment-shaped in two
   of three formats, but it is never treated as one), `$schema` (that's
   specifically the JSON spelling — say Schema hint for the format-neutral
   concept).
   ```
   Add one clarifying sentence to the end of the existing **Conversion**
   entry's second paragraph (currently line 172, ending "...but **comments
   carry across** with the target marker."): append
   `` — except a **Schema hint**, which is recognized and re-authored in the
   target's own convention rather than carried across verbatim (see Schema
   hint, § Schema).``

6. **Tests — `crates/confy-core/src/model/convert.rs`, inside the existing
   `#[cfg(test)] mod tests` block** (add after the existing
   `json_to_toml_scope_tables` test, currently ending at line 1142; reuse
   the existing `convert_str(src, from, to) -> ConvertResult` helper,
   currently lines 1108–1110). All six directed format pairs, one drop
   case, one no-op regression, and three tests for the merged-comment-block
   correctness fix from step 4:
   - `json_schema_hint_becomes_yaml_modeline` (Json→Yaml): source
     `{ "$schema": "./s.json", "a": 1 }\n`; assert `r.text` starts with
     `"# yaml-language-server: $schema=./s.json\n"`, does not contain the
     substring `"$schema"` anywhere else, and `r.warnings.is_empty()`.
   - `json_schema_hint_becomes_toml_first_line` (Json→Toml): same source;
     assert `r.text` starts with `"#:schema ./s.json\n"`.
   - `yaml_modeline_becomes_json_schema_field` (Yaml→Json): source
     `"# yaml-language-server: $schema=./s.json\na: 1\n"`; assert `r.text`
     deserializes (`serde_json::from_str`) to an object whose `"$schema"`
     member is `"./s.json"`, and the raw text contains no
     `"yaml-language-server"` substring.
   - `toml_schema_hint_becomes_yaml_modeline` (Toml→Yaml): source
     `"#:schema ./s.json\nport = 1\n"`; assert `r.text` starts with
     `"# yaml-language-server: $schema=./s.json\n"`.
   - `yaml_modeline_becomes_toml_first_line` (Yaml→Toml): source
     `"# yaml-language-server: $schema=./s.json\nport: 1\n"`; assert
     `r.text` starts with `"#:schema ./s.json\n"`.
   - `toml_schema_hint_becomes_json_schema_field` (Toml→Json): source
     `"#:schema ./s.json\nport = 1\n"`; assert `r.text` deserializes to an
     object whose `"$schema"` member is `"./s.json"`.
   - `yaml_modeline_dropped_with_warning_on_sequence_root_to_json`
     (Yaml→Json, sequence root — nowhere for `$schema` to attach): source
     `"# yaml-language-server: $schema=./s.json\n- 1\n- 2\n"`; assert
     `r.text == "[\n  1,\n  2\n]\n"` (unaffected) and `r.warnings ==
     vec!["schema hint dropped: JSON $schema requires an object root".to_string()]`.
   - `no_hint_present_is_unaffected`: reuse the existing `toml_to_json_basic`
     source (no hint); assert `r.warnings.is_empty()` and the text matches
     that existing test's expectation unchanged.
   - `toml_hint_line_split_from_merged_leading_comment_block` (Toml→Yaml,
     merged-block regression): source
     `"#:schema ./s.json\n# keep me\nport = 1\n"` (no blank line between the
     hint and the next comment ⇒ they merge into one `Item::Comment`);
     assert `r.text` starts with `"# yaml-language-server: $schema=./s.json\n"`
     and also contains the substring `"keep me"` (the unrelated comment
     line must survive, not be swept away with the hint).
   - `yaml_modeline_not_first_leading_comment_still_translates` (Yaml→Json,
     merged-block regression): source
     `"# header\n# yaml-language-server: $schema=./s.json\nport: 1\n"` (the
     modeline is the second of two merged leading comment lines); assert
     `r.text` contains `"\"$schema\": \"./s.json\""`, contains `"header"`
     (preserved as a `//` JSONC comment), and does not contain
     `"yaml-language-server"` anywhere (the stale modeline must not remain
     alongside the freshly injected one).
   - `yaml_modeline_after_blank_line_still_translates` (Yaml→Toml,
     merged-block regression): source
     `"# header\n\n# yaml-language-server: $schema=./s.json\nport: 1\n"` (a
     blank line splits the two leading comments into separate `Item`s, so
     the modeline is `items[1]`, not `items[0]`); assert `r.text` starts
     with `"#:schema ./s.json\n"`, contains `"header"`, and does not
     contain `"yaml-language-server"`.

7. **Integration test — `crates/confy-core/tests/session_headless.rs`**, in
   the existing "Pointer convert" section (after
   `dispatch_set_convert_path_then_run_writes`, currently ending at line
   1394): add `dispatch_convert_run_carries_toml_schema_hint_to_json`, using
   the file's existing `toml_session(src)` helper:
   ```rust
   #[test]
   fn dispatch_convert_run_carries_toml_schema_hint_to_json() {
       let mut s = toml_session("#:schema ./s.json\na = 1\n");
       s.dispatch(Intent::SetCursor(vec![]));
       s.dispatch(Intent::OpenConvert);
       s.dispatch(Intent::SetConvertFormat(DocFormat::Json));
       let snap = s.dispatch(Intent::ConvertRun);
       let (_, text) = snap.convert_write.expect("convert produced a write, no warnings expected");
       assert!(text.contains("\"$schema\": \"./s.json\""), "json output:\n{text}");
   }
   ```
   This exercises the full `Session::convert_run` → `convert::convert` path,
   confirming no session/dispatch-level wiring is missing beyond steps 2–4.

No other crate (confy-tui, confy-ffi, confy-tauri, web/) needs changes:
`ConvertResult.warnings` and `Session::convert_run`'s existing "non-empty
warnings ⇒ Confirm step" flow (`crates/confy-core/src/session/session.rs:1162-1188`,
unchanged) already surface any new warning to every host.

## Critical files & anchors

- `crates/confy-core/src/schema/hints.rs:45-73` — `detect_yaml`/`detect_toml`,
  the extraction site for step 1's shared predicates.
- `crates/confy-core/src/model/convert.rs:34-49` — `convert()`, the edit
  site for step 3.
- `crates/confy-core/src/model/convert.rs:17-20` — import block, step 2.
- `crates/confy-core/src/model/value.rs:30-42` — exact `Item`/`Value` shapes
  used by every new helper in step 4.
- `docs/reference/CONTEXT.md:167-176,290-300` — Conversion and Schema
  sections, edit sites for step 5.

## Verification

1. `cargo test -p confy-core convert::tests --lib` — the 11 new unit tests
   from step 6 plus all pre-existing `convert::tests` (regression: default
   rendering of documents with no hint must stay byte-identical).
2. `cargo test -p confy-core --test schema_headless detect_hint` — confirms
   step 1's `detect_yaml`/`detect_toml` refactor is behavior-preserving.
3. `cargo test -p confy-core --test session_headless dispatch_convert` — the
   new integration test from step 7 plus the two pre-existing
   `dispatch_set_convert_*` tests.
4. `cargo test -p confy-core` (full crate suite) — confirms no other test
   (e.g. `tests/serde_roundtrip.rs`, which dispatches
   `Intent::SetConvertFormat`) regresses from the `convert()` signature/body
   change.
5. Manual end-to-end spot check tying back to the reported gap: run
   `cargo test -p confy-core json_schema_hint_becomes_yaml_modeline -- --nocapture`
   and confirm it passes — the exact scenario from the original evaluation
   (JSON `$schema` field silently becoming a stray YAML data key) now
   produces a proper `# yaml-language-server: $schema=...` modeline instead.
6. `cargo test -p confy-core toml_hint_line_split_from_merged_leading_comment_block yaml_modeline_not_first_leading_comment_still_translates yaml_modeline_after_blank_line_still_translates -- --nocapture` —
   confirms the merged-comment-block correctness fix from step 4 (over-deletion
   of unrelated adjacent comments, and under-detection of a non-first-line
   YAML modeline) actually holds, not just the common-case tests.
