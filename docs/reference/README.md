# Reference

Current behavior only. Anything historical — a superseded design, a shipped plan, a resolved
investigation — lives in [`../spec/`](../spec/README.md), [`../plan/`](../plan/README.md),
[`../debug/`](../debug/README.md), or [`../audit/`](../audit/README.md), never here.

## Model

- **[glossary.md](glossary.md)** — canonical vocabulary; read first. Node/Root/Branch/Leaf/
  Scalar/Comment, key literal vs decoded key, read-only and opaque nodes, `DocFormat`, the
  Value tree, schema terms, and the full KIND-tag vocabulary.
- **[MUTATIONS.md](MUTATIONS.md)** — how each operation behaves: insert/move legality,
  `e` block-edit scope, per-`Mutation` splice mechanics (including multiline-array layout),
  and the kind-switch (`K`) rules.
- **[BEHAVIOR_MATRIX.md](BEHAVIOR_MATRIX.md)** — how a node's nesting **scope** governs every
  editing behavior, normalized across TOML/JSON/YAML: tables A/B/C, the design criteria, the
  `ConfigDocument` facet layer, and the scope-independent invariants.

## Surfaces

- **[TUI.md](TUI.md)** — ratatui TUI mechanics: rendering, editing, comments, navigation,
  filters, multi-select, clipboard, overlays, i18n.
- **[WEBUI.md](WEBUI.md)** — Web UI + WASM FFI contract, web-native architecture, touch UI,
  deployment.
- **[CHROME.md](CHROME.md)** — header/toolbar chrome: button inventory, responsive fold order,
  per-host trimming. Shared by desktop and touch.
- **[KEYMAP.md](KEYMAP.md)** — TUI ↔ Web keyboard bindings, plus the deliberate divergences.
- **[HOST_PARITY.md](HOST_PARITY.md)** — the index of **every** deliberate TUI ↔ web/touch/
  VS Code/Tauri divergence, not just keys: one line each, pointing at the doc that owns the
  detail. Read it before adding host-specific behavior.
- **[TAURI.md](TAURI.md)** — desktop + Android app shell (`confy-tauri`).
- **[VSCODE.md](VSCODE.md)** — VS Code extension host (`editors/vscode`).

## Cross-cutting

- **[MESSAGES.md](MESSAGES.md)** — Notice/prompt/diagnostics message system, unified across
  every host.
- **[ROW_STATE_MODEL.md](ROW_STATE_MODEL.md)** — row cursor/selection/clipboard state model,
  unified across TUI/desktop/touch.
- **[RELEASES.md](RELEASES.md)** — where each build ships, how it gets there, current status.
- **[changelog/](changelog/README.md)** — archived release notes. The root `CHANGELOG.md`
  carries `[Unreleased]` plus the current series only; completed series move here verbatim.

## Machine-checked claims

These files carry claims a test enforces, so they cannot silently drift:

| File | Claim | Test |
|---|---|---|
| `KEYMAP.md` | the normal-mode key table | `crates/confy-tui/src/tui/keys.rs` tests + `web/keymap-parity.spec.mjs` |
| `MESSAGES.md` | the `core.*` notice-key severity table | `crates/confy-core/src/session/notice.rs::severity_of_covers_the_full_catalog_table` |
| `MUTATIONS.md` | multiline-array insert/delete layout | `crates/confy-core/tests/roundtrip.rs` |
| `BEHAVIOR_MATRIX.md` | §8's cross-format invariants (Remark's own-line rule, the `MutateError` variant taxonomy, single-value `Replace` fragments) | `crates/confy-core/tests/format_parity.rs` |

See also [`../adr/`](../adr/README.md) for the decision records behind this shape.
