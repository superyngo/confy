# Pointer-drop `PasteSlot` alignment — probe (RESOLVED 2026-09-01)

> ✅ **Resolved.** The fix landed as commit `5e5d9d1` (ADR
> [0010](../../adr/0010-pointer-drops-resolve-through-pasteslot.md)); this folder is the
> frozen evidence, not a live task.

`probe3.mjs` is the scratch harness that measured the bug against the **real wasm core**
(the same `crates/confy-ffi/pkg` module the web/touch UIs load), not a unit-test double.
It builds a TOML session, then for every visible row × pointer band compares:

1. the destination web/touch's grip drag now sends (`pointer_slot` verbatim), vs.
2. what an armed cut+paste released at the same pixel resolves (`effective_paste_slot`),

asserting byte-identical documents and identical notices — plus the two reported
symptoms directly: the gap under an expanded `[b]` (must land as `[b]`'s first child,
no notice) and the `Into` band on `Format::Inline` containers (TOML inline table/array,
YAML flow map/sequence), including a real copy-drag onto one.

Before the fix, 7 of the 25 row×band combinations disagreed (the post-fix run prints
`✓ drag and paste agree at EVERY band`). The equality is now pinned in-repo by
`move_selection_to_and_paste_agree_for_every_pointer_band`
(`crates/confy-core/tests/session_headless.rs`); this probe remains for ad-hoc
re-verification against a rebuilt bundle.

Run:

```sh
cd crates/confy-ffi && wasm-pack build --target web --out-dir pkg
node docs/superpowers/debug/2026-09-01-pointer-drop-pasteslot-probe/probe3.mjs
```

(The script resolves the wasm from `crates/confy-ffi/pkg/` relative to the repo root.)

`probe.mjs`/`probe2.mjs` (same `/tmp` scratch session) were the earlier
symptom-measurement passes and are superseded by this file; not kept.
