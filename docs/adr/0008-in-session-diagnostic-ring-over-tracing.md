# Diagnostics are an in-Session ring buffer, not `tracing`

The 2026-08-21 message-system design (see
`docs/superpowers/specs/2026-08-21-message-system-design.md` §7) adds a
developer-facing diagnostics layer as a bounded ring of `DiagEvent` records
inside `Session` itself (capacity 256, English-only, five event kinds:
dispatch / mutation / schema / convert / host_notice), exported through three
small surfaces — the TUI `~` overlay, FFI `diag_log()`, and web `?diag=1` —
instead of adopting the `tracing` or `log` crates. The reason is confy's core
architecture: `Session` is a pure, host-free value that is fully unit-testable
headlessly and compiles unchanged for TUI, wasm, and VS Code; a `tracing`
global dispatcher/subscriber is process-wide mutable state that fights exactly
that property (leaking across tests, differing per host, invisible to
`Session`-level assertions), while the ring keeps every diagnostic testable as
ordinary data and costs one `VecDeque` instead of a dependency chain in the
wasm binary. Reversal is genuinely possible — wrap the ring's record points in
a trait later — but call sites across five event kinds would all move at once,
which is why the choice is recorded here.

## Considered options

- **`tracing` crate** — rejected: global subscriber state vs the pure
  `Session` value model; wasm32 binary growth; no need for third-party
  backends, sampling, or span trees for what is a five-kind event tap.
- **`log` crate** — rejected: equally global, less structured; would still
  need a per-test capture mechanism the ring gives for free.
- **Host-side `eprintln!`/`console.log`** — rejected: not present on
  wasm32-unknown-unknown for stdout, unstructured, untestable, and different
  per host — the ring keeps one cross-host vocabulary.
