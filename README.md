# confy

A cross-platform Terminal User Interface (TUI) for editing structured configuration files.

Modeled on [wenv](https://github.com/superyngo/wenv)'s navigation/selection/editing UX, but
targeting **markup config formats** (TOML first; YAML and JSON planned) and **single-file editing**.

## Usage

```
confy <file.toml>
```

Opens the file in an interactive TUI tree editor. On save (`w` or `Ctrl+s`) the file is
written back with comments, key order, and formatting fully preserved (byte-identical round-trip
for unmodified subtrees).

## Scope

- **Single-file editing** — one file per session; no multi-file workspace.
- **TOML-first** — round-trip via `toml_edit`; YAML/JSON planned but not in MVP.
- **Round-trip preserving** — comments, key order, and whitespace are kept intact on save.

## Keybindings

| Key | Action |
|-----|--------|
| `j` / `↓` | Move cursor down |
| `k` / `↑` | Move cursor up |
| `PgUp` / `PgDn` | Page up / down |
| `Home` / `End` | First / last row |
| `Enter` / `Space` | Expand/collapse branch, or open leaf detail (scroll with ↑/↓/PgUp/PgDn/Home/End) |
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
| `z` | Undo |
| `y` | Redo |
| `/` | Filter (fuzzy search) |
| `?` | Help |
| `Esc` | Cancel prompt / clear filter / close overlay |
| `w` / `Ctrl+s` | Save |
| `q` | Quit (confirms if there are unsaved changes) |

## License

MIT
