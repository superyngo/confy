# confy

A cross-platform Terminal User Interface (TUI) for editing structured configuration files.

Modeled on [wenv](https://github.com/superyngo/wenv)'s navigation/selection/editing UX, but
targeting **markup config formats** (TOML, JSON/JSONC, and a lossless YAML subset) and **single-file editing**.

## Usage

```
confy <file.toml>
```

Opens the file in an interactive TUI tree editor. On save (`w` or `Ctrl+s`) the file is
written back with comments, key order, and formatting fully preserved (byte-identical round-trip
for unmodified subtrees).

### Convert between formats

```
confy convert <in> <out> [--from <fmt>] [--to <fmt>] [--yes]
```

Converts a config file to another format (`toml` / `json` / `jsonc` / `yaml`), defaulting the
formats from the file extensions. Comments carry across; notation/style differences are
**normalized to the target's default style with an up-front lossy-warning list** (on a TTY you
confirm y/n, otherwise pass `--yes`). A conversion **aborts and writes nothing** when the source
holds something the target can't represent (`null` → TOML, or a YAML opaque node → any target).
**The source file is never modified.** The same flow is available inside the TUI on the Root node
via `C`.

## Web UI

confy also ships a pointer-first **web UI** that drives the same headless core
(`confy-core`) through a WebAssembly build — full tree editing, kind/format
switching, conversion, and lossless save, in the browser. There's a dedicated
touch layout for coarse-pointer devices.

**Live demo: <https://confy.turkeyang.net/>**

Build/run locally: `cd web && npm install && node build.mjs` then `node serve.mjs`
(see [WEBUI.md](WEBUI.md)). The hosted site is deployed from `web/` via Cloudflare
Workers Builds (`web/cf-build.sh` + `wrangler.toml`).

## Desktop app

The same web UI ships as a native desktop app (Tauri shell, `crates/confy-tauri`) with
native open/save dialogs, in-place writes, and CLI-arg file open. Releases include:

- **macOS**: `confy-desktop-macos-{aarch64,x86_64}.dmg`. Unsigned/un-notarized — on first
  launch macOS blocks it; right-click → Open (or `xattr -cr /Applications/confy.app`).
- **Windows**: `confy-desktop-windows-x86_64.exe` — portable, no install (the UI is embedded
  in the binary; requires the WebView2 runtime, preinstalled on Win11 and near-universal on
  Win10). Unsigned, so SmartScreen shows an "unknown publisher" warning: More info → Run anyway.
  A `confy-desktop-windows-x86_64.msix` is also published for Microsoft Store submission
  (see `crates/confy-tauri/msix/STORE.md`); it is unsigned by design — the Store signs it.

## Format support

| Format | Status | Notes |
|--------|--------|-------|
| TOML | Full | Lossless CST via taplo/rowan; all TOML 1.0 features |
| JSON / JSONC | Supported | Lossless hand-rolled rowan CST; `//` line comments become first-class nodes; `/* */` block comments are read-only nodes; trailing commas accepted on parse |
| YAML | Subset | Lossless hand-rolled rowan CST; single document (optional leading `---`); block + single-line flow maps/seqs; 5 scalar styles (plain/single/double/`\|`/`>`); `#` comments; YAML 1.2 core-schema typing (no datetime). Out-of-subset constructs (anchors, aliases, `<<` merge, tags, multi-line flow) become **read-only opaque nodes** (`[opaq ]`); multi-document files are rejected at load |

## Scope

- **Single-file editing** — one file per session; no multi-file workspace.
- **Multi-format** — TOML and JSON/JSONC fully supported; a lossless YAML subset (out-of-subset constructs degrade to read-only opaque nodes).
- **Round-trip preserving** — comments, key order, and whitespace are kept intact on save.

## Keybindings

| Key | Action |
|-----|--------|
| `j` / `↓` | Move cursor down |
| `k` / `↑` | Move cursor up |
| `PgUp` / `PgDn` | Page up / down |
| `Home` / `End` | First / last row |
| `Enter` / `Space` | Expand/collapse branch, or open leaf detail (scroll with ↑/↓/PgUp/PgDn/Home/End) |
| `i` | Toggle the detail/info popup for any node (incl. branches; shows kind + child count) |
| `0` | Collapse all |
| `9` | Expand all |
| `s` | Toggle selection on the cursor row |
| `Shift+↑` / `Shift+↓` | Extend range selection |
| `←` / `→` | Toggle a bool, or step a number by ±1 (preserves base/precision) |
| `a` | Add node (inserts `new_field = ""` below the cursor, opens the inline editor) |
| `e` | Edit value — inline for a plain scalar, `$EDITOR` for nested array/table |
| `E` | Edit any node in `$EDITOR` (force external) |
| `d` | Delete selected node(s) |
| `c` | Copy selected node(s) |
| `x` | Cut selected node(s) |
| `v` | Paste clipboard |
| `m` | Move selected node(s) |
| `r` | Remark (toggle comment-out) |
| `K` | Kind switch (scalar notation / container style) |
| `C` | Convert document to another format (Root node) |
| `z` | Undo |
| `y` | Redo |
| `/` | Filter (fuzzy search) |
| `?` | Help |
| `Esc` | Cancel prompt / clear filter / close overlay |
| `w` / `Ctrl+s` | Save |
| `q` | Quit (confirms if there are unsaved changes) |

## License

MIT
