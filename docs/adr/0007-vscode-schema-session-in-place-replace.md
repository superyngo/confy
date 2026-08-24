---
status: implemented (2026-08-21)
---

# VS Code native-editor schema session updates in place via `ApplyReplace`, never rebuilt per keystroke

## Context

The VS Code schema-hints design (`docs/superpowers/specs/2026-08-21-vscode-schema-hints-design.md`)
needs a live `ConfySession` per open native-editor document that both
Diagnostics and Hover can query. The existing `DocumentSymbolProvider`
(outline/breadcrumbs) precedent constructs a brand-new `ConfySession` on
every request and throws it away — a valid pattern there because outline
computation is pure and synchronous (parse text, walk tree, done).

Schema support is not pure and synchronous: loading a schema is a stateful
async round trip (`detect_and_request_schema()` → host fetches/reads →
`apply_schema_text()`, which compiles a `jsonschema::Validator`), and per
`session.rs:1489`/`1499`, **core does no dedup of its own** —
`detect_and_request_schema()` unconditionally returns a fetch request
whenever an in-document hint exists, and `apply_schema_text()`
unconditionally recompiles the validator every time it's called, regardless
of whether the schema text is unchanged. If the native-editor host rebuilt a
fresh `ConfySession` and redid the full detect → fetch → compile round trip
on every debounced `onDidChangeTextDocument` firing (as the outline
provider's pattern would suggest), it would re-fetch and recompile the
schema on every few keystrokes while the user types.

Separately, `Mutation::Replace { path: vec![] }` (whole-document reparse)
already exists and is exercised today by TOML/YAML's `reparse_document`/
`commit_reparse` and exposed to hosts via `Intent::ApplyReplace { path, text }`
(used today for the external-`$EDITOR` edit-resolution flow, but its handler
does not require that context — it works for any `path`, including empty).

## Decision

`SchemaSessionManager` keeps **one persistent `ConfySession` per open
document** for its whole lifetime (open → close), not a fresh session per
edit. Each edit feeds the document's latest full text into the *same*
session via `session.dispatch({ApplyReplace: {path: [], text}})` rather than
constructing a new `ConfySession`. Schema detection
(`Intent::DetectSchema`) is re-run after every successful reparse, but the
host compares the detected source against what it already knows is loaded
(`ManagedDoc.loadedSchemaSource`) and only fetches/reloads on a real change
— since core itself won't skip that work.

## Considered options

- **Rebuild-from-scratch per reparse** (mirroring the outline provider) —
  rejected: even with a host-side cache of the fetched schema *text* (to
  avoid redundant I/O), `apply_schema_text()` would still be called, and
  therefore the `jsonschema::Validator` would still be recompiled, on every
  debounced keystroke. For a large schema this is a real, avoidable cost the
  outline provider's pure-parse case never had to pay.
- **Rebuild-from-scratch per reparse, with the host tracking "already this
  exact schema" to skip re-dispatching `SchemaLoaded` too** — closes the
  compile-cost gap, but still discards and reconstructs the whole session
  (tree, cursor/selection state if ever added, etc.) on every keystroke for
  no benefit over keeping the session alive; more moving parts than just
  keeping one session and calling `ApplyReplace`.
- **In-place `ApplyReplace`** (chosen) — the compiled `SchemaState` survives
  edits untouched; only `revalidate_schema()` (cheap: re-`validate()`, no
  recompilation) reruns after a successful edit.

## Consequences

- Mid-edit invalid syntax (`ApplyReplace` → `MutateError::Fragment`) leaves
  the session's tree — and therefore its schema violations — at the last
  successfully-parsed state; `self.error` is set but `self.schema` is
  untouched. The host must not blindly re-display those (now
  position-stale) violations against the live, further-edited buffer — the
  design's Q7 decision is to clear Diagnostics for that document until the
  next successful reparse, rather than show drifted positions.
- The host now owns dedup logic (`loadedSchemaSource` comparison) that core
  deliberately does not provide — this is consistent with core's existing
  "confy-core is fs-free, hosts resolve I/O" boundary, but it is new
  *dedup* responsibility, not just I/O, that every future stateful host
  adopting schema support will need to reimplement the same way.
- This is the first native-editor-host precedent to keep a `ConfySession`
  alive across host-observed text changes; a future feature that also needs
  live session state (not just per-request pure computation) should follow
  the same shape (persistent session + `ApplyReplace`), not the outline
  provider's throwaway-session shape.
