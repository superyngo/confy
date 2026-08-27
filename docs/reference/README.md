# Developer reference docs

Entry point for confy's developer-facing reference documentation. Root-level
docs (`README.md`, `CLAUDE.md`, `CHANGELOG.md`, `RELEASES.md`) stay at the
repo root; everything below lives here.

- **[CONTEXT.md](CONTEXT.md)** — canonical model glossary (Node/Root/Branch/Leaf/
  Scalar/Comment, Mutation mechanics, Insert/move legality, Schema vocabulary).
- **[BEHAVIOR_MATRIX.md](BEHAVIOR_MATRIX.md)** — the full nested-behavior matrix
  across TOML/JSON/YAML backends.
- **[TUI.md](TUI.md)** — ratatui TUI-specific mechanics.
- **[WEBUI.md](WEBUI.md)** — Web UI & WASM FFI contract, web-native architecture.
- **[CHROME.md](CHROME.md)** — header/toolbar chrome single source of truth: button
  inventory, responsive fold order, and per-host (VS Code/Tauri desktop) trimming rules,
  shared by desktop and touch.
- **[TAURI.md](TAURI.md)** — desktop + mobile app shell (`confy-tauri`).
- **[VSCODE.md](VSCODE.md)** — VS Code extension host (`editors/vscode`).
- **[MESSAGES.md](MESSAGES.md)** — Notice/diagnostics message system, unified
  across all hosts.
- **[ROW_STATE_MODEL.md](ROW_STATE_MODEL.md)** — row cursor/selection/clipboard
  state model, unified across TUI/desktop/touch.
- **[PORTING.md](PORTING.md)** — Headless Core extraction & multi-platform port
  design record.

See also [`../adr/`](../adr/) for Architecture Decision Records and
[`../superpowers/`](../superpowers/) for design specs, plans, and audits.
