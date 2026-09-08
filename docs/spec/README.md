# Specs

Design records written before implementation.

Every file here is a historical record: frozen once it lands, dated by when it was
written, never rewritten. Current behavior lives in
[`../reference/`](../reference/README.md).

A `Shipped (date)` / `Resolved (date)` marks **when the record was frozen**; the release that
carried the change is in `CHANGELOG.md`.

## In progress

Nothing open.

## Landed

| Date | Document | Status |
|---|---|---|
| 2026-05-27 | [confy — Single-File TUI Config Editor (Design Spec)](2026-05-27-confy-toml-config-editor-design.md) | Shipped (2026-08-12) |
| 2026-06-12 | [Multi-format backends: JSON/JSONC + YAML (+ document-level conversion)](2026-06-12-multiformat-backends-design.md) | Shipped (2026-08-12) |
| 2026-06-17 | [Headless Core extraction & multi-platform port](2026-06-17-headless-core-port.md) | Shipped (2026-08-30) |
| 2026-07-09 | [Help/About panel, header info button, overlay z-index fix, unified Open popup](2026-07-09-help-about-open-panel-design.md) | Shipped (2026-08-28) |
| 2026-07-15 | [confy VS Code extension — M1 design](2026-07-15-vscode-extension-design.md) | Shipped (2026-07-16) |
| 2026-08-10 | [JSON Schema Support — Design](2026-08-10-json-schema-support-design.md) | Shipped (2026-08-12) |
| 2026-08-20 | [VS Code `DocumentSymbolProvider` (Outline / Breadcrumbs) — Design](2026-08-20-vscode-outline-provider-design.md) | Shipped (2026-08-28) |
| 2026-08-21 | [Message System Integration — Design](2026-08-21-message-system-design.md) | Shipped (2026-08-28) |
| 2026-08-21 | [VS Code Schema Hints (Diagnostics + Hover) — Design](2026-08-21-vscode-schema-hints-design.md) | Shipped (2026-08-28) |
| 2026-08-28 | [JSON/JSONC comment write-gate removal — Design](2026-08-28-json-jsonc-comment-gate-removal-design.md) | Shipped (2026-08-28) |
| 2026-08-30 | [Action menu — centralized node operations across desktop, touch, and TUI](2026-08-30-action-menu-design.md) | Shipped (2026-08-30) |

See also [`../adr/`](../adr/README.md) for the decisions this material produced.
