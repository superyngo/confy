# confy

A cross-platform Terminal User Interface (TUI) for editing structured configuration files.

Modeled on [wenv](https://github.com/superyngo/wenv)'s navigation/selection/editing UX, but
targeting **markup config formats** (TOML first; YAML and JSON planned) and **single-file editing**.

## Status

🚧 Early planning. See `docs/superpowers/specs/` for the design spec.

## Concept

- Parse a config file into an **entry tree** (objects/tables become expandable nodes; scalars are leaves)
- **Multi-level toggle** tree navigation
- **Round-trip preserving** edits (comments, key order, formatting kept intact)
- wenv-style keys: navigation, selection, `$EDITOR` editing, cut/copy/paste, undo/redo, fuzzy filter

## License

MIT
