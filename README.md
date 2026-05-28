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
| `←` / `→` | Collapse / expand branch |
| `Enter` / `Space` | Expand or open detail for node |
| `0` | Collapse all |
| `9` | Expand all |
| `s` + `Shift+↑/↓` | Extend selection up/down |
| `n` | New node |
| `e` | Edit node value (opens `$EDITOR`) |
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
| `w` / `Ctrl+s` | Save |
| `q` | Quit |

## License

MIT
