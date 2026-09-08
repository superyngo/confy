# Plans

Task-by-task implementation plans derived from a spec.

Every file here is a historical record: frozen once it lands, dated by when it was
written, never rewritten. Current behavior lives in
[`../reference/`](../reference/README.md).

A `Shipped (date)` / `Resolved (date)` marks **when the record was frozen**; the release that
carried the change is in `CHANGELOG.md`.

## In progress

- [2026-09-09-open-follow-ups.md](2026-09-09-open-follow-ups.md) — the single live backlog:
  every recorded-but-unfixed item, with evidence and acceptance criteria · `In progress`

This is the one record here that is *not* frozen on landing — see its own scope note.

## Landed

| Date | Document | Status |
|---|---|---|
| 2026-05-27 | [confy MVP Implementation Plan](2026-05-27-confy-toml-config-editor-plan.md) | Shipped (2026-08-12) |
| 2026-06-08 | [Clipboard Mode Separation Implementation Plan](2026-06-08-clipboard-mode-separation.md) | Shipped (2026-08-12) |
| 2026-06-08 | [CST backend migration — comments as real, independent nodes](2026-06-08-cst-backend-migration.md) | Shipped (2026-06-10) |
| 2026-06-08 | [Move-aware cut, exact-position reorder, and comment clipboard — Implementation Plan](2026-06-08-move-comment-clipboard.md) | Shipped (2026-08-12) |
| 2026-06-09 | [Cross-layer node ops + precise line-paste](2026-06-09-cross-layer-ops-and-line-paste.md) | Shipped (2026-08-12) |
| 2026-06-10 | [Handover — KIND column header, 40% column position, help legend + scrollable help](2026-06-10-kind-column-help-legend-handover.md) | Shipped (2026-08-12) |
| 2026-06-10 | [Handover — "reconstruct proposal" increments on the CST backend](2026-06-10-reconstruct-increments-handover.md) | Shipped (2026-08-12) |
| 2026-06-12 | [Multi-format backends — handover prompts](2026-06-12-multiformat-handover-prompts.md) | Shipped (2026-08-12) |
| 2026-06-12 | [Phase 1: Backend Abstraction Implementation Plan](2026-06-12-phase1-backend-abstraction.md) | Shipped (2026-08-12) |
| 2026-06-13 | [JSON/JSONC Backend Implementation Plan](2026-06-13-json-jsonc-backend.md) | Shipped (2026-06-13) |
| 2026-06-13 | [YAML Subset Backend Implementation Plan](2026-06-13-yaml-subset-backend.md) | Shipped (2026-06-13) |
| 2026-06-24 | [Web-native UI/UX redesign for confy](2026-06-24-web-native-ui.md) | Shipped (2026-08-12) |
| 2026-07-09 | [Help/About Panel, Overlay Fix, Info Button, Unified Open Popup — Implementation Plan](2026-07-09-help-about-open-panel.md) | Shipped (2026-08-12) |
| 2026-07-11 | [i18n (multi-language) plan — TUI + Web/Desktop](2026-07-11-i18n-plan.md) | Shipped (2026-08-12) |
| 2026-07-12 | [Desktop (Tauri) native menu plan — File/Edit/View/Help](2026-07-12-desktop-menu-plan.md) | Shipped (2026-08-12) |
| 2026-07-12 | [Mobile extension spec — Tauri iOS/Android shell over the confy web UI](2026-07-12-mobile-tauri-spec.md) | Shipped (2026-07-15) |
| 2026-07-13 | [Mobile M1 plan — Android sideload APK (Tauri v2)](2026-07-13-mobile-m1-android-plan.md) | Shipped (2026-07-15) |
| 2026-07-15 | [confy VS Code Extension M1 Implementation Plan](2026-07-15-vscode-extension-m1.md) | Shipped (2026-07-15) |
| 2026-07-16 | [VS Code M1.5 — Shared Dirty State (CustomTextEditorProvider) Implementation Plan](2026-07-16-vscode-m1-5-shared-dirty-state.md) | Shipped (2026-08-12) |
| 2026-07-17 | [Breadcrumb Navigation Bar Implementation Plan](2026-07-17-breadcrumb-nav.md) | Shipped (2026-08-12) |
| 2026-08-06 | [Mobile M2 plan — Android Save As + "Open with" chooser visibility](2026-08-06-mobile-m2-saveas-fileassoc-plan.md) | Shipped (2026-08-06) |
| 2026-08-10 | [JSON Schema Support Implementation Plan](2026-08-10-json-schema-support.md) | Shipped (2026-08-12) |
| 2026-08-11 | [Audit Remediation — Implementation Plan](2026-08-11-audit-remediation-plan.md) | Shipped (2026-08-12) |
| 2026-08-11 | [Web Code Audit Remediation — Implementation Plan](2026-08-11-web-code-audit-remediation-plan.md) | Shipped (2026-08-12) |
| 2026-08-17 | [ADR 0004: Unified Clipboard/Move Targeting Implementation Plan](2026-08-17-adr-0004-unified-clipboard-targeting.md) | Shipped (2026-08-27) |
| 2026-08-18 | [Row-State Visual Language (Phase 1) Implementation Plan](2026-08-18-row-state-visual-language-phase1.md) | Shipped (2026-08-27) |
| 2026-08-18 | [Row-State Visual Language (Phase 2) Implementation Plan](2026-08-18-row-state-visual-language-phase2.md) | Shipped (2026-08-27) |
| 2026-08-18 | [Row-State Visual Language (Phase 3) Implementation Plan](2026-08-18-row-state-visual-language-phase3.md) | Shipped (2026-08-27) |
| 2026-08-18 | [Row-State Visual Language (Phase 4) — Implementation Plan](2026-08-18-row-state-visual-language-phase4.md) | Shipped (2026-08-27) |
| 2026-08-18 | [Row-State Visual Language (Phase 5) — Implementation Plan](2026-08-18-row-state-visual-language-phase5.md) | Shipped (2026-08-27) |
| 2026-08-20 | [Schema Warning Indicators — Type Filter Facet + Collapsed-Branch Marker](2026-08-20-schema-warning-indicators-plan.md) | Shipped (2026-08-27) |
| 2026-08-20 | [VS Code `DocumentSymbolProvider` (Outline / Breadcrumbs) Implementation Plan](2026-08-20-vscode-outline-provider-plan.md) | Shipped (2026-08-27) |
| 2026-08-21 | [Message System Integration Implementation Plan](2026-08-21-message-system-integration.md) | Shipped (2026-08-27) |
| 2026-08-21 | [VS Code Schema Hints (Diagnostics + Hover) Implementation Plan](2026-08-21-vscode-schema-hints.md) | Shipped (2026-08-27) |
| 2026-08-27 | [Carry the schema hint's writing convention across convert/save-as](2026-08-27-convert-schema-hint-plan.md) | Shipped (2026-08-27) |
| 2026-08-28 | [Comment-advisory follow-up issues](2026-08-28-comment-advisory-followup-issues.md) | Resolved (2026-08-28) |
| 2026-08-28 | [JSON/JSONC comment write-gate removal Implementation Plan](2026-08-28-json-jsonc-comment-gate-removal.md) | Shipped (2026-08-28) |
| 2026-08-28 | [Plan — make a key's *authored spelling* a first-class projection output](2026-08-28-key-repr-first-class-literal.md) | Shipped (2026-08-28) |
| 2026-08-28 | [JSON/JSONC parser simplification — single source of truth](2026-08-28-json-jsonc-parser-simplification-ssot.md) | Shipped (2026-09-09) |
| 2026-09-01 | [Nudge redesign: remove boolean nudge, gate numeric wheel/gesture nudge to inline-edit mode](2026-09-01-nudge-redesign.md) | Shipped (2026-09-02) |
| 2026-09-01 | [Web UI tree: PageUp/PageDown navigation](2026-09-01-web-ui-tree-paging.md) | Shipped (2026-09-01) |
| 2026-09-02 | [Help overlay keymap alignment & visual polish](2026-09-02-help-overlay.md) | Shipped (2026-09-02) |
| 2026-09-02 | [confy web UI built-in sample overhaul](2026-09-02-web-ui-samples.md) | Shipped (2026-09-02) |
| 2026-09-07 | [TOML datetime kind switch: `K` picks among the 4 datetime types, one consolidated `a` entry](2026-09-07-datetime-kind-switch.md) | Shipped (2026-09-07) |
| 2026-09-07 | [Trailing blank lines are an explicit, undoable node operation](2026-09-07-trailing-blank-lines.md) | Shipped (2026-09-07) |

See also [`../adr/`](../adr/README.md) for the decisions this material produced.
