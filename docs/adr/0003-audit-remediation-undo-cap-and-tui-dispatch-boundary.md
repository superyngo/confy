---
status: implemented
---

# Undo history is capped and lossy; TUI routes only mutations through `dispatch(Intent)`

Two independent decisions from the 2026-08-11 audit remediation plan, bundled here because
both permanently trade a small amount of capability/purity for a bounded, predictable system,
and both would otherwise puzzle a future reader staring at the code with no context.

**Undo history cap.** `Session::History` (`session/state.rs`) stores one full serialized-text
snapshot per mutation, uncapped — memory grows as edit count × document size, unbounded for the
life of a session. We're capping `History.past` at a fixed 200 entries via a `VecDeque` ring
buffer, evicting the oldest snapshot once full. 200 is a constant, not a setting: no CLI flag,
no config, no per-host UI — the same value on TUI, web, touch, and Tauri. This is a real
trade-off (a session doing >200 edits silently loses its earliest undo steps, with no
in-product signal that the boundary was crossed) chosen over the reversible-but-costlier
alternative of compressed/diffed snapshots, because 200 full-document edits is generous for
config-file editing and diffed snapshots are meaningfully more implementation complexity for a
benefit no user has asked for.

**TUI `dispatch(Intent)` routing boundary.** The TUI (`confy-tui/src/tui/mod.rs`) calls
`Session` methods directly rather than through `Session::dispatch(Intent)` the way the
Web/Tauri hosts do — an audit finding, since it hand-duplicates cross-cutting dispatch logic
(shift-select reset, `ToggleExpand`'s branch/leaf decision). The obvious fix reads as "route
everything through `dispatch()`," but the TUI crate has ~495 `session.`-qualified references,
not the audit's estimated ~40 — the overwhelming majority are read-only state queries
(`app.session.mode`, `.tree`, `.cursor`, …) that `dispatch()` itself performs internally to do
its job, not calls that belong on the Intent wire. We're routing **only the ~40 genuinely
mutating calls** (navigation, selection, schema-enum, inline-edit, and mutation/undo commands)
through `Intent`; every read-only query stays a direct call. This is a permanent architectural
line, not a staging step toward "eventually everything goes through dispatch" — a future
engineer finding `app.session.cursor` called directly two lines above an `Intent::CursorDown`
dispatch should read that as intentional, not incomplete.

## Update (2026-08-11, later session)

`Session::dispatch(Intent) -> SessionSnapshot` was split into a cheap
`apply(Intent) -> ApplyOutcome` (mutation + transient signals only, no row
rebuild or render snapshot) plus a thin `dispatch()` wrapper —
`dispatch()`'s public behavior is unchanged. This was a prerequisite gap
this ADR didn't anticipate: `dispatch()` unconditionally calls
`compute_rows()` (O(visible nodes)) and builds the full `SessionSnapshot`
on every call, while the TUI's current `App` facade deliberately skips its
own row rebuild for pure-navigation methods (`cursor_down`/`up`/`home`/
`end`/`page_up`/`page_down`) specifically because it's unneeded work on the
hottest input path. Routing those intents through the old single-shape
`dispatch()` would have reintroduced, at the input layer, the exact
"rebuild everything every frame" cost Task 16 (2026-08-11,
`perf(tui+web): window tree rendering to the viewport`) had just eliminated
at the render layer.

A future Task 13 attempt should route the TUI's ~50 structural/mutating
call sites (filter, type-filter, kind-switch, schema-enum, inline-edit,
mutations, undo/redo, convert, lifecycle — everything that already calls
`App::rebuild_rows` today) through `apply()` and reuse the TUI's own
existing `rebuild_rows()` afterward, same as now. Pure navigation
(`CursorDown`/`Up`/`Home`/`End`/`PageUp`/`PageDown`) can also call `apply()`
directly (still cheap — no snapshot built), but stays a `Session`-side
direct-call-shaped intent regardless, since it never needed a row rebuild.
This also means the TUI's App-facade split (`app.rs`, landed after this
ADR) already funnels ~90% of the crate's mutating `Session` calls through
~90 named delegate methods, not scattered across the event loop — the
495-call-site count above measured total `session.`-qualified references
including read-only queries reached through that facade, not raw scattered
mutation call sites needing individual conversion. The actual conversion
surface for a future attempt is smaller than this ADR's number suggests.

## Resolution (2026-08-11, Task 13 implemented)

Done, per the plan above. `confy-tui/src/tui/app.rs`'s wrapper methods now
call `Session::apply(Intent::_)` internally wherever an `Intent` variant is
an exact behavioral match — ~65 of them (navigation, filter, type-filter,
kind-switch, convert, detail, help, selection, inline-edit, mutations,
undo/redo, escape, prompt). Every method kept its existing signature, so no
call site outside `app.rs` changed; `cargo test -p confy-tui` (178 tests,
all pre-existing) passed unchanged both before and after, confirming the
swap is behaviorally inert. Two call sites *in* `mod.rs` had real
hand-duplicated logic (not just a raw method call) and were rewritten to
call `apply()` directly, deleting the duplicate:
- `ToggleExpand`'s branch/leaf decision (`r.is_branch { toggle_expand() }
  else { open_detail() }`) — now `apply(Intent::ToggleExpand)` decides,
  `mod.rs` only keeps its own `is_branch` read to skip the row rebuild when
  a leaf opened Detail instead (Detail doesn't change the row list).
- `Quit`'s `if confirm_quit() {} else if quit_requested() {}` gate — now
  `apply(Intent::QuitRequested)`, whose `ApplyOutcome::quit` replaces the
  two-call dance. `App::confirm_quit()`/`quit_requested()` stay (still
  called individually by `app.rs`'s own tests), just no longer from `mod.rs`.

Left un-routed, each for a real semantic reason, not oversight:
- `App::toggle_expand()` itself stays the raw, unconditional
  `Session::toggle_expand()` — paste mode's `Into`-slot handler in `mod.rs`
  needs the dumb toggle; swapping it for `apply(Intent::ToggleExpand)` would
  add a leaf/branch check that path never had.
- `convert_pick_format`, `edit_clamp_scroll` — the `Intent` variant's core
  handling is deliberately host-divergent (`ConvertPickFormat` hardcodes a
  `None` stem since fs-free core has no source path; `EditClampScroll` is a
  no-op since the Web host's DOM scrolls itself, but the TUI's terminal-
  width clamp is real session state only this host needs).
- `apply_replace`/`apply_edit_comment`, `begin_inline_edit`, `edit_node`,
  `save`, `lang_picker_commit`, `open_lang_picker` — no exact-match `Intent`
  exists (they're either test-only helpers, host-specific $EDITOR/fs I/O
  with no fs-free core equivalent, or bundle extra host logic — e.g. `save`
  does the real `std::fs::write` the fs-free `Intent::Save` assumes the host
  already did) or the smart routing they'd need duplicates a decision made
  one layer up in `mod.rs`'s own `EditNode` handling. Converting any of
  these would be a behavior change, not a refactor.

Verified beyond the unit suite: `cargo clippy --workspace --all-targets -D
warnings` and `cargo test --workspace` both clean; a live TUI smoke run
(real binary, real terminal keys) confirmed leaf-ToggleExpand opens Detail,
branch-ToggleExpand expands + shows children, clean-doc Quit exits
immediately, and dirty-doc Quit enters the confirm prompt with `y`
completing the exit — the exact four paths this ADR's two dedup fixes touch.

## Considered options

- **Undo: uncapped (status quo)** — rejected: unbounded memory growth is the audit's own
  Medium-severity finding; no upside to leaving it unbounded.
- **Undo: configurable cap** — rejected for now: adds a new cross-surface setting (TUI/web/
  touch/Tauri) to design and wire for a value nobody has asked to tune. Revisit if a real user
  need for deeper undo surfaces.
- **TUI: route 100% of `Session` calls through `dispatch(Intent)`** — rejected: read-only state
  queries have no `Intent` equivalent and manufacturing one for all ~455 of them would bloat the
  `Intent` enum and the wasm wire contract with variants no other host needs, for zero behavior
  change (`dispatch()` reads the same state internally).
- **TUI: leave 100% direct, drop the routing task entirely** — rejected: leaves the audit's
  actual finding (hand-duplicated cross-cutting mutation logic) unaddressed.
