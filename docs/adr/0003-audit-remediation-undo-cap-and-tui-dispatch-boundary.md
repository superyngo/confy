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
