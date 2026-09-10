# Specs

Design records written before implementation.

Every file here is a historical record: frozen once it lands, dated by when it was
written, never rewritten. Current behavior lives in
[`../reference/`](../reference/README.md).

A `Shipped (date)` / `Resolved (date)` marks **when the record was frozen**; the release that
carried the change is in `CHANGELOG.md`.

## In progress

_None._

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
| 2026-09-10 | [Web UI draws the document Root row (TUI alignment) — Design evaluation](2026-09-10-web-root-row-alignment.md) | Shipped (2026-09-10) |

## Prototype assets

Two static mockups that later specs and reference docs cite as the *verbatim* visual source.
They are frozen alongside the records that consumed them, and have no `Status:` line of their
own because they are not Markdown records:

| Asset | Ported by |
|---|---|
| [2026-06-24-design_index_model.html](2026-06-24-design_index_model.html) | the desktop web UI — `web/index.html`/`web/style.css` carry its `<style>` block verbatim ([plan](../plan/2026-06-24-web-native-ui.md)) |
| [2026-06-26-web-respons-migrate-to-touch-ready.html](2026-06-26-web-respons-migrate-to-touch-ready.html) | the touch UI — `web/touch/` ([WEBUI.md § Touch UI](../reference/WEBUI.md)) |

See also [`../adr/`](../adr/README.md) for the decisions this material produced.
