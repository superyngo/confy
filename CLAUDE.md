# CLAUDE.md — confy developer guide

## Build & test commands

```bash
cargo build                   # compile
cargo test                    # unit + integration tests
cargo clippy -- -D warnings   # lint (must be clean before commit)
cargo fmt                     # format
cargo fmt --check             # check formatting without modifying
cargo run -- <file.toml>      # run against a TOML file
cargo bench -p confy-core     # perf harness (no criterion; plain main() + medians)
# Bigger synthetic document. `--bench perf` is required: without it the args
# reach the lib test binary first, which rejects `--nodes`.
cargo bench -p confy-core --bench perf -- --nodes 5000

# Web / touch UI (from web/) - NOT covered by `cargo test`
cd web
npm run typecheck             # tsc --noEmit
npm run build                 # esbuild bundles + wasm-pack COPY (never a wasm rebuild:
                              # a confy-core change reaches the browser only after
                              # `cd crates/confy-ffi && wasm-pack build --target web`.
                              # build.mjs warns when pkg/ is older than the .rs sources)
npm test                      # plain-Node spec suite (node run-tests.mjs)
# The wasm command channel end-to-end (Intent -> SessionSnapshot):
cd crates/confy-ffi && wasm-pack build --target web && node functional_smoke.mjs
```

**Two test conventions worth knowing.** (1) The **web suite is a plain-Node harness** —
no framework: each `*.spec.mjs` esbuild-bundles the TS module under test and tallies
`check(name, cond)` calls, so render modules must stay importable without the wasm glue
(hence `highlight.ts`'s `setFuzzyMatcher` injection instead of a `pkg/` import).
(2) A **CLI integration test that asserts message text must pin `--lang en`** — with no
flag the binary resolves the language from the *real* `~/.config/confy/config.toml`, so an
unpinned English assertion passes in CI and fails on a zh-TW machine. MESSAGES.md §5.5.

## Release process

**Three version files + CHANGELOG must all move together for every release** —
`.github/workflows/release.yml`'s `verify-versions` job hard-fails the tagged
build if any of them disagree with the tag:

- `Cargo.toml` (`[workspace.package].version` — covers all Rust crates:
  confy-core, confy-tui, confy-ffi, confy-tauri)
- `web/package.json` (`.version`)
- `editors/vscode/package.json` (`.version` — also regenerate
  `editors/vscode/package-lock.json`'s root version via
  `npm install --package-lock-only` in `editors/vscode/`, so `npm ci` doesn't
  warn on a stale lockfile)
- `CHANGELOG.md` must contain a `## [vX.Y.Z]` section for the tag. It holds `[Unreleased]`
  plus the **current series only** — completed series are archived verbatim under
  `docs/reference/changelog/` (its index says when and how). Never archive the series the
  next tag belongs to.

Bump all four in the same release commit, before tagging. Never tag with only
`Cargo.toml` updated.

**`web/package-lock.json`'s root version is deliberately NOT managed.** It was resynced to
`1.3.2` once (2026-09-18, after drifting to `0.18.1` for many releases) and is left alone from
here: nothing consumes it — `verify-versions` doesn't check it, `web/build.mjs` reads
`package.json`, and `npm ci` only warns when the *dependency* tree disagrees, which
`npm install` already keeps in sync. Do not add it to the release checklist; the
`editors/vscode` lock above stays on the list only because `npm ci` runs in that package's
VSIX packaging path and warns there.

**Also update the MSIX Store listing's ReleaseNotes** at
`crates/confy-tauri/msix/listings/listingData-9PLCJGQ3C654.csv` — set the `ReleaseNotes`
column to describe the new version in the same release commit.

## Commit citations

A doc row that names the commit closing it must name a **reachable** hash. The trap: write the
row with a placeholder, `sed` the real hash in, then `git commit --amend` — the amend rewrites
the commit, so the citation points at an unreachable object. Nine citations across
`CHANGELOG.md` and four plan docs were dangling this way on 2026-09-15. Either land the commit
and cite it from the next one, or verify before pushing:

```sh
rg -o '`[0-9a-f]{7,10}`' --no-filename CHANGELOG.md docs/**/*.md | tr -d '`' | sort -u \
  | xargs -I{} sh -c 'git cat-file -t {} >/dev/null 2>&1 && \
      { git merge-base --is-ancestor {} HEAD || echo "unreachable: {}"; }'
```

On 2026-09-18 that leaves exactly five hits — the `root-row-alignment` ones in
`docs/debug/2026-09-11-root-row-alignment-retrospective.md`.

Hashes on a *deliberately abandoned* branch are fine when the doc says so (the
`root-row-alignment` record cites five such commits from that six-commit branch on
purpose) — keep that branch alive.

## Architecture — where each contract is documented

This file is the **conduct** file: commands, release mechanics, and repo rules. It deliberately
does **not** restate reference content (`wens-dev-principles docs 3`). Start at
[`CONTEXT.md`](CONTEXT.md), the documentation index, and read
[`docs/reference/glossary.md`](docs/reference/glossary.md) before touching model code — the terms
are not interchangeable with their synonyms (use **Node**, never "Entry").

The shape in one paragraph: a **headless, filesystem-free core** (`confy-core`) owns the
document model and all editor state; every host — TUI, web, Tauri desktop/Android, VS Code —
drives it through one command channel (`Session::dispatch(Intent) -> SessionSnapshot`) and owns
only its own I/O and presentation. Three concrete backends (TOML via `taplo`, JSON/JSONC and a
YAML subset via hand-rolled lossless parsers) sit behind one `ConfigDocument` trait, all on
`rowan` green trees, all atomic-commit, so an untouched file round-trips byte-identically.

| Topic | Where it is specified |
|---|---|
| **Which file holds what** — workspace shape, the full module map, the host file-I/O boundary, per-host build/packaging, and the `taplo` dependency surface | [`docs/reference/ARCHITECTURE.md`](docs/reference/ARCHITECTURE.md) |
| Vocabulary: Node/Root/Branch/Leaf/Scalar/Comment, key literal vs decoded key, read-only & opaque nodes, `DocFormat`, the `Value` tree, schema terms, KIND tags | [`docs/reference/glossary.md`](docs/reference/glossary.md) |
| Every `Mutation` variant's mechanics, insert/move legality, `e` block-edit scope, multiline-array layout, kind switch (`K`) rules | [`docs/reference/MUTATIONS.md`](docs/reference/MUTATIONS.md) |
| How nesting **scope** governs each editing behavior across the three backends; the inline-vs-`$EDITOR` boundary; the `ConfigDocument` facet layer | [`docs/reference/BEHAVIOR_MATRIX.md`](docs/reference/BEHAVIOR_MATRIX.md) |
| TUI rendering, editing, comments, navigation, filters, multi-select, clipboard, overlays | [`docs/reference/TUI.md`](docs/reference/TUI.md) |
| WASM FFI wire contract (`Intent`/`SessionSnapshot`/`ViewRow`), web-native architecture, touch UI, deployment | [`docs/reference/WEBUI.md`](docs/reference/WEBUI.md) |
| Header/toolbar button inventory, fold order, per-host trimming | [`docs/reference/CHROME.md`](docs/reference/CHROME.md) |
| Keyboard bindings and the deliberate TUI↔Web divergences | [`docs/reference/KEYMAP.md`](docs/reference/KEYMAP.md) |
| Every **other** deliberate host divergence (rendering, row state, editing, chrome, capabilities), one row each | [`docs/reference/HOST_PARITY.md`](docs/reference/HOST_PARITY.md) |
| Notice/prompt/diagnostics message system, severity table, per-host channels | [`docs/reference/MESSAGES.md`](docs/reference/MESSAGES.md) |
| Row cursor/selection/clipboard state model and its modal lock | [`docs/reference/ROW_STATE_MODEL.md`](docs/reference/ROW_STATE_MODEL.md) |
| Desktop + Android shell: native menu, file I/O, recent files, the Android picker plugin | [`docs/reference/TAURI.md`](docs/reference/TAURI.md) |
| VS Code extension host and its `TextDocument` protocol | [`docs/reference/VSCODE.md`](docs/reference/VSCODE.md) |
| Distribution channels, triggers, current status per platform | [`docs/reference/RELEASES.md`](docs/reference/RELEASES.md) |
| Why the shape is what it is | [`docs/adr/README.md`](docs/adr/README.md) |

**Two invariants worth stating here, because breaking either is a review failure rather than a
doc lookup.** (1) `confy-core` is **filesystem-free at runtime** — no `fs`/`process`/`env`/
`tempfile`, no terminal deps; the sole constructor is `from_str`/`AnyDocument::from_str_as`, and
`crates/confy-core/tests/no_fs_gate.rs` enforces it. The host owns all file I/O
(`confy_tui::load_document` / `write_document`, which also handle the UTF-8 BOM and the atomic
temp-file rename). (2) Every mutation is **atomic and semantically validated before commit** —
edited on a `clone_for_update` copy, committed only on success, so a failure leaves the document
untouched.

## Known Risks

**`taplo` is unmaintained upstream**, and `rowan =0.15.18` is exact-pinned to match taplo's
internal version. The **decision and its trigger** (do not migrate; `.github/workflows/rust-ci.yml`'s
`cargo audit` step flags a `rowan`/`taplo`/`ahash` advisory → vendor the used surface) is one
entry in the *Watching* section of
[`docs/plan/BACKLOG.md`](docs/plan/BACKLOG.md), which is
the single place every deliberately deferred dependency decision lives (so is `ureq`'s pin at
2.x). The **measured surface** and the ~1,240-LOC vendoring estimate are in
[`docs/reference/ARCHITECTURE.md`](docs/reference/ARCHITECTURE.md) §Dependency surface, which
also records what that measurement covers and when it was last taken. Do not restate the
decision here — three copies of it drifted once already.

## Terminology

See [`docs/reference/glossary.md`](docs/reference/glossary.md) for the canonical vocabulary.
Key rule: use **Node** (not "Entry"). Subtypes are **Root**, **Branch node**, **Leaf node**,
**Scalar**, and **Comment**. The operation that toggles a live Node to/from a Comment is
**Remark** (key `r`). Introducing a new term means adding its glossary entry in the same commit.
