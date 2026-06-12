# Multi-format backends: JSON/JSONC + YAML (+ document-level conversion)

**Date:** 2026-06-12
**Status:** approved design, pre-implementation
**Scope decisions (user-approved):**

1. Both new backends are **lossless** — an untouched file round-trips byte-identically,
   same guarantee as the TOML CST backend.
2. **YAML is a restricted subset with read-only degradation**: constructs outside the
   subset load and display but reject mutation (`Unsupported`, document untouched).
3. Cross-format support is **document-level conversion only** ("save as other format"),
   with an explicit lossy warning. No cross-format clipboard in v1.
4. **XML is out of scope entirely** (not even a placeholder phase).
5. **JSONC is supported** by the JSON backend (comments via `//`); introducing the first
   comment into a pure `.json` file prompts for confirmation.
6. Backend dispatch is an **enum wrapper** (`AnyDocument`), not generics or trait objects.

Delivered as four phases, each implemented in its own session from its own handover
prompt. Phase boundaries are hard: each phase leaves `main` green (`cargo test`,
`cargo clippy -- -D warnings`, `cargo fmt --check`) and shippable.

---

## Phase 1 — backend abstraction (pure refactor, TOML behavior unchanged)

Goal: remove the three TOML leaks between the TUI and the document layer. **No new
format ships.** Acceptance: every existing test (golden projections, roundtrip
integration, app/ui unit tests) passes **unmodified** except for mechanical renames
(`toml:` field → `fragment:`).

### 1.1 `AnyDocument`

New `src/model/any_doc.rs`:

```rust
pub enum AnyDocument {
    Toml(CstDocument),
    // Json(JsonDocument)  — Phase 2
    // Yaml(YamlDocument)  — Phase 3
}
impl ConfigDocument for AnyDocument { /* match-delegate every method */ }
```

`App.doc: Option<AnyDocument>` (app.rs:47), `tui::run` constructs via a new
`AnyDocument::load_detected(path, format_override)` that owns format detection.
Tests that build a `CstDocument` directly wrap it (`AnyDocument::Toml(doc)`).

### 1.2 Format-neutral `Mutation`

- `Mutation::Insert { toml, .. }` / `Replace { toml, .. }` → field renamed `fragment`.
- `MutateError::Fragment` message text de-TOML-ed ("invalid fragment: …"); each backend
  supplies the format name in the message it constructs.
- `Mutation`, `Target`, `OnCollision`, `MutateError` otherwise unchanged — they are
  already format-neutral (paths are `Seg` sequences; comments are `Seg::Index` nodes).

### 1.3 Kind-switch becomes a capability query

Today `open_kind_switch` (app.rs) hard-codes the TOML notation lattice. Move the
knowledge to the backend:

```rust
// ConfigDocument
fn kind_options(&self, path: &[Seg]) -> Vec<KindTarget>;
```

returning the *legal-by-kind* targets for the node (current kind excluded); positional
legality (capture rules etc.) stays inside `apply(ConvertKind)` exactly as today.
`KindTarget` remains ONE superset enum shared by all formats; Phases 2–3 add variants
(see matrices below). The TOML impl returns exactly what `open_kind_switch` computes
today — the popup rendering and Enter/Esc flow do not change.

### 1.4 Format facet on the trait

```rust
pub enum DocFormat { Toml, Json, Yaml }   // model-level (cli::Format maps onto it)

// ConfigDocument
fn format(&self) -> DocFormat;
fn comment_prefix(&self) -> &'static str;     // "#" / "//"
fn supports_comments(&self) -> bool;          // false only for pure JSON before the
                                              // JSONC-upgrade confirmation (Phase 2)
```

Used by: comment validation in the inline comment editor and `InsertComment`
(`comment_prefix` replaces the hard-coded `#` checks), the title bar, help rendering
(1.6), and the JSONC prompt (Phase 2).

### 1.5 CLI

`cli::Format` grows `Json | Yaml`; `detect_format` maps `.json`/`.jsonc` → Json,
`.yaml`/`.yml` → Yaml; both bail "coming in a later release" until their phase lands.
`--format` accepts the new names with the same bail.

### 1.6 Help / keys plumbing

`keys.rs` help text gains a per-format filter hook: each help row carries an optional
`DocFormat` mask. Phase 1 marks the rows that will become format-dependent
(remark/comment ops, `K`) but all rows stay visible for TOML — rendering changes only
when a non-TOML doc is loaded (Phases 2–3 set the masks).

### 1.7 Out of scope for Phase 1

`type_filter.rs` facets, `type_tag`, projection — untouched (they keyed off
`NodeKind`/`Format`/`KeySign` which stay as-is). `ScalarType::Null` is added in
Phase 2, not here.

---

## Phase 2 — JSON/JSONC backend

### 2.1 Parser: hand-rolled rowan CST

New module family mirroring the TOML trio:

```
src/model/json/
  mod.rs        re-exports
  syntax.rs     SyntaxKind enum + rowan Language impl
  parse.rs      lexer + recursive-descent parser → rowan green tree (lossless)
  doc.rs        JsonDocument: load/serialize/apply (atomic commit, same pattern as cst_doc)
  project.rs    CST → NodeTree projection + golden tests
  edit.rs       splice helpers, one fn per Mutation variant
```

Rowan is added as a **direct dependency** (currently transitive via taplo; pin the same
version taplo uses). The grammar is JSON + JSONC extensions:

- `//` line comments and `/* */` block comments as real tokens;
- trailing commas **accepted on parse** (lossless keeps them); confy's own splices never
  emit them;
- everything else per RFC 8259. A parse error rejects the load with line/col.

Atomicity, `validate_semantics`-style backstop (duplicate keys in an object →
`Collision`), and the `clone_for_update`/commit-on-success pattern are copied from
`cst_doc.rs`.

### 2.2 Projection mapping

| JSON construct        | NodeKind                  | Format                  | KeySign |
| --------------------- | ------------------------- | ----------------------- | ------- |
| object                | `Table`                   | `Inline` / `Multiline`  | `Quoted` (its own key) |
| array                 | `Array`                   | `Inline` / `Multiline`  | `Quoted` |
| string                | `Scalar(String)`          | `Plain`                 | `Quoted` |
| number (no `.`/`eE`)  | `Scalar(Integer)`         | `Decimal`               | `Quoted` |
| number (otherwise)    | `Scalar(Float)`           | `Plain` / `Exponent`*   | `Quoted` |
| `true`/`false`        | `Scalar(Bool)`            | `Plain`                 | `Quoted` |
| `null`                | `Scalar(Null)` **(new)**  | `Plain`                 | `Quoted` |
| standalone `//` line(s) | `Comment`               | `Plain`                 | `None`  |
| end-of-line `//`      | owner's `trailing_comment` | —                      | —       |
| `/* */` block         | `Comment`, **read-only**  | `Plain`                 | `None`  |

\* `Format::Exponent` is a **new variant** (TOML's K float toggle currently detects
exponent from text because Format lacks it — Phase 2 adds the variant; the TOML
projection may adopt it in a later cleanup, not in this phase).

New: `ScalarType::Null`; `type_tag` slot `[S:null]`; `NodeKind::Scalar(Null)` is a
leaf with `value = Some("null")`. Array elements/`None`-key rules identical to TOML
(`Seg::Index` over the full child sequence, comments share the slot space).

Root array documents (`[1, 2]` at top level) are legal JSON: the Root node's children
are the elements, same as any array.

**Read-only nodes.** `Node` gains `pub read_only: bool` (default false). A `/* */`
block comment projects read-only: `e`/`d`/`x`/`r` on it → status "read-only node
(block comment)"; copy is allowed (fragment = raw text). This flag is also the
mechanism Phase 3's opaque YAML nodes use.

### 2.3 Behavior matrix (JSON/JSONC)

| Operation | Behavior |
| --------- | -------- |
| navigation, `/` filter, `f` type filter, multi-select, expand/collapse | unchanged (operate on NodeTree) |
| `e` inline / `E` `$EDITOR` | same dispatch rules; fragments are JSON fragments (`"key": value` for keyed, bare value for elements) |
| `a` add | seeds `"key": ""` under object, `""` element in array |
| value `null` | typed literally in the inline editor; type-change prompt fires (str → null etc.) |
| Rename (Tab) | rewrites the quoted key; no dotted semantics — `foo.x` is just a key containing a dot (no type change prompt) |
| `d` / `x` / `c` / `v` | identical flow; comma/indent normalization handled by `json/edit.rs` splices |
| array element ↔ object member moves | same unpack/pack rules as TOML (`{ "k": v }` wraps a keyed fragment into an array; an object element pasted into an object unpacks; bare value into object gets `"placeholder"`) |
| `r` remark | comments out as `// "key": value,` (one `//` per line for multiline extents); unremark parses the text back as a member |
| comments | `//` only for authored comments; `InsertComment` validates every line starts with `//`; `/* */` read-only (see 2.2) |
| pure `.json` + first comment-introducing op (`r`, comment paste, InsertComment) | `Mode::Prompt(JsoncUpgrade)` — "this makes the file JSONC; proceed? y/n" (pattern copied from ArrayUpgrade). One confirmation per session; after `y`, `supports_comments()` is true for the doc |
| `K` kind options | object/array: Inline ↔ Multiline; float: Plain ↔ Exponent; string/int/bool/null: none |
| N/A (absent from JSON) | dotted keys, `[A/T]`, datetimes, int radix, string notations, multiline strings (encoded `\n` only) |

`KindTarget` additions: **reuse where the notation name applies, add one variant.**
`ArrayInline/ArrayMultiline` and `FloatPlain/FloatExponent` are notation-named, not
TOML-named, and apply as-is; JSON objects use `TableInline` plus a new
`TableMultiline`. `TableDotted/TableScope/ArrayOfTables/Int*/String*` are simply never
returned by `JsonDocument::kind_options`.

### 2.4 KIND column (JSON)

Same 12-column scheme. Key sign: `(Q)` for keyed nodes, `(-)` keyless. Type slot:
`[T/I]`/`[T/M]` object, `[A/I]`/`[A/M]` array, `[S:str ]`, `[S:int ]`, `[S:flt ]`,
`[S:bool]`, `[S:null]`, comment same tag as TOML. `[T/M]` is **new** (TOML's scope
table stays `[T/S]`; a multiline JSON object is `[T/M]`). `type_filter::classify`
extended arm-for-arm with `type_tag` (existing inverse-function invariant test extended
to the new tags); the `f` popup shows only facets reachable in the loaded format
(facet list keyed off `DocFormat`).

### 2.5 Help

JSON-mode help hides/N-A-marks: dotted/AoT-specific lines, `r` line changes to the
`//` description, `K` line lists the JSON options. Driven by the Phase 1 masks.

---

## Phase 3 — YAML subset backend

### 3.1 Supported subset (fully editable)

- Single document (optional leading `---`).
- Block mappings and block sequences, arbitrarily nested; flow mappings `{ }` and flow
  sequences `[ ]` (single-line flow fully editable; multi-line flow read-only in v1).
- Scalar styles: plain, single-quoted, double-quoted, literal `|`, folded `>`
  (with `+`/`-` chomping indicators preserved).
- `#` comments: standalone lines → Comment nodes (consecutive lines merge, same rule
  as TOML), end-of-line → `trailing_comment`.
- Scalar typing per YAML 1.2 core schema: `null`/`~`, `true`/`false`, int
  (dec / `0x` / `0o`), float (plain / exponent / `.inf` / `.nan`), else string.
  **No datetime type** (looks-like-a-date scalars are strings).

### 3.2 Out-of-subset → opaque read-only nodes

Anchors `&a`, aliases `*a`, merge keys `<<:`, tags `!!x`/`!x`, block scalars with
exotic headers, multi-line flow collections: the **whole value span** parses as one
opaque token sequence and projects as a read-only node (`read_only: true`,
`NodeKind` per its top-level shape if cheaply known, else `Scalar(String)` with the
raw text as value; KIND tag `(…)[opaq ]`). Mutations touching an opaque node or
inserting *into* one → `MutateError::Unsupported`. Mutations elsewhere in the file
work normally — the opaque span is spliced around, never re-rendered.

**Multi-document files (`---` more than once) are rejected at load** with a clear
message (v1).

### 3.3 Parser + splice engine

```
src/model/yaml/  — same six-file layout as json/
```

Hand-rolled lossless lexer/parser onto rowan. Indentation is part of the token stream
(INDENT tokens), so serialization stays plain token concatenation. The splice layer's
core is an **indent engine**: an inserted/moved node is re-indented to the destination
depth (re-prefix every line of the fragment; literal/folded scalar bodies shift with
their header). Comma handling exists only in flow collections.

**Gate task (first task of the phase): parser spike.** Corpus of ≥10 real-world files
(docker-compose with anchors, GitHub Actions workflows, k8s manifests, simple configs)
must satisfy: parse succeeds, `serialize()` is byte-identical, out-of-subset constructs
are correctly fenced as opaque. **If the spike fails structurally, stop and re-plan**
(fallback candidates: tree-sitter-yaml for parse + span-based editing) — do not push
through.

### 3.4 Behavior matrix (YAML)

| Operation | Behavior |
| --------- | -------- |
| navigation/filter/select | unchanged |
| `e`/`E` dispatch | single-line plain/quoted scalars + single-line flow collections inline; literal/folded scalars, block collections, Root → `$EDITOR` |
| `a` add | `key: ""` in a mapping, `- ""` element in a sequence |
| Rename | rewrites the key scalar in place; quoting preserved/added as the new key requires |
| `d`/`x`/`c`/`v` | block nodes captured with their full indented extent; paste re-indents to destination (3.3); keyed↔element pack/unpack rules mirror TOML/JSON: a keyed fragment into a sequence becomes a block mapping under `- ` (the idiomatic YAML form, not a flow `{ k: v }`); an element mapping pasted into a mapping unpacks into members |
| `r` remark | `# key: value` line-prefix per line of the node's extent; unremark re-parses |
| comments | `#`, same prefix rules as TOML |
| `K` kind options | mapping/sequence: Block ↔ Flow (flow target rejects held comments and multi-line members, mirroring TOML's array-collapse rule); string: plain ↔ single ↔ double ↔ literal ↔ folded (plain target rejected when the content needs quoting; literal/folded targets only for multi-line-capable contexts); int: dec ↔ hex ↔ oct; float: plain ↔ exponent |
| opaque nodes | all mutations `Unsupported`; copy allowed (raw text fragment) |

`KindTarget` additions: `Flow`, `Block` (containers), `StringPlain`, `StringSingle`,
`StringDouble` (YAML quoting), `StringLiteralBlock`, `StringFolded`. TOML's
`StringLiteral` (= `'…'`) is distinct from YAML single-quoted in escaping rules but the
enum variant is per-notation-name; backends interpret their own variants —
`kind_options` only ever returns variants its format defines.

`Format` additions: YAML block collections use a **new `Format::Block`**; flow
collections reuse the existing `Inline`. Scalar styles get **new variants** —
`SingleQuoted`, `DoubleQuoted`, `LiteralBlock`, `Folded` (plain stays `Plain`) — rather
than overloading TOML's `Literal`/`Multiline*` names, whose escaping semantics differ.

### 3.5 KIND column (YAML)

Key sign: `(B)` plain key, `(Q)` quoted key, `(-)` keyless. Type slot: `[T/B]`/`[T/F]`
mapping block/flow, `[A/B]`/`[A/F]` sequence, scalars `[S:…]` as elsewhere (incl.
`[S:null]`), `[opaq ]` for opaque, comment tag shared. `classify`/`type_tag` extended
in lockstep; YAML facet set in the `f` popup includes the Block/Flow group and the five
string styles.

---

## Phase 4 — document-level conversion

### 4.1 Surface

- CLI: `confy convert <in> <out>` — formats from extensions, `--from/--to` overrides.
  Prints the lossy-warning list and requires `--yes` (or interactive y/n on a TTY).
- TUI: an action on the Root node (key chosen during implementation; help-listed)
  prompting for target format + output path, showing the same warning list.

### 4.2 Mechanism

New `src/model/value.rs` (`Value` enum: Null/Bool/Int/Float/Str/Datetime/Seq/Map —
**ordered** map, plus per-node attached comments: leading comment block + trailing
comment) and `src/model/convert.rs`:

```
source backend --to_value()--> Value --render(DocFormat, default style)--> String
```

Each backend implements `to_value()` (projection-adjacent walk) and a `render_value()`
default-style serializer (TOML: scope tables + bare keys; JSON: 2-space multiline,
JSONC comments only if any exist; YAML: block style, plain scalars where legal).

### 4.3 Loss & legality matrix

| Construct | → TOML | → JSON(C) | → YAML |
| --------- | ------ | --------- | ------ |
| comments | kept (`#`) | kept (`//`, makes JSONC) | kept (`#`) |
| notation/styles (radix, string style, inline-vs-block, dotted, AoT) | normalized to default style — **warned** | same | same |
| TOML datetime | — | string — **warned** | string — **warned** |
| `null` | **abort**, listing every null path | — | — |
| YAML opaque nodes present | **abort** ("file contains unsupported YAML constructs") | abort | abort |

Abort = no output file written; warnings = listed up front, conversion proceeds after
confirmation. The source document is never modified by `convert`.

---

## Testing (every phase)

- **Golden projection tests** per backend (pattern from `cst_project.rs`).
- **Byte-identical roundtrip fixtures**: `tests/fixtures/*.{json,jsonc,yaml}` —
  load → serialize → diff; YAML corpus includes the opaque-construct files.
- **Mutation unit tests** mirroring `cst_edit.rs` suites (insert/delete/replace/
  rename/move/remark/comment/convert-kind per format).
- **`classify` ↔ `type_tag` inverse invariant** extended to new tags.
- **Conversion tests** (Phase 4): matrix rows above, abort cases, comment carry-over.
- Phase 1 specifically: the *unmodified* existing suite is the acceptance gate.

## Documentation (every phase)

`CHANGELOG.md` Unreleased entry; `CLAUDE.md` architecture + module map; `CONTEXT.md`
glossary (Opaque node, JSONC upgrade, DocFormat); help text; `README.md` format
support table; `Cargo.toml` description line.

## Risks

1. **YAML splice indent engine** — highest risk; mitigated by the spike gate (3.3) and
   by the flow-multiline + multi-doc read-only/reject fences.
2. **rowan version pinning** against taplo's transitive copy (one `rowan` in the tree).
3. **`Format`/`KindTarget` enum growth** — variants are per-notation and
   format-scoped via `kind_options`; the inverse-invariant test keeps tag/filter/popup
   in sync.
4. **JSONC `/* */`** — read-only fence keeps v1 small; full block-comment editing can
   come later without model changes.
