# TOML datetime kind switch: `K` picks among the 4 datetime types, one consolidated `a` entry

✅ **Shipped — historical reference.** Landed 2026-09-07 (`d6c923b`, `76c602c`, `5ec5511`);
the frozen decision is **ADR 0012**. See `CHANGELOG.md` for current behavior; this plan is
kept for context, not as a live task list. One deviation from the plan as written: Task 1's
`retype_fills_a_missing_component_and_reports_it` expected `LocalTime → LocalDate` to report
only `FilledDate`, but it correctly reports `[FilledDate, DroppedTime]` — a fill and a drop
can occur in the same switch, and the label names both. The test was corrected, not the code.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make TOML's four datetime types (`[D:odt]`/`[D:ldt]`/`[D:ldat]`/`[D:ltim]`)
mutually convertible from the `K` key, disclosing dropped and auto-filled components before
commit and confirming the type change; and collapse the `a` Add-type picker's four datetime
rows into one.

**Architecture:** A cross-type datetime switch is a **value `Replace`**, not a
`Mutation::ConvertKind`. `K` on a datetime node opens the existing `Mode::SchemaEnum` picker
(`from_schema: false`) instead of `Mode::KindSwitch`, with each option's *value* being the
fully-rendered target literal and its *label* disclosing the loss/fill. `schema_enum_commit`
already routes through `edit_commit`, so the existing `PromptKind::TypeChange` confirmation
gate applies for free (`inline_edit.rs:1001-1010` documents exactly this).

**Net new machinery: one pure module.** No new `Mutation` variant, no new `KindTarget`, no new
`Mode`, no new `PromptKind`, no wire-contract (`web/types.ts`) change, no host code change on
any of the four hosts — every host already renders `ModeView::SchemaEnum` and
`PromptView::TypeChange`.

**Tech Stack:** Rust (`confy-core`), the existing `Mode::SchemaEnum` popup, `i18n/*.json`.

**Spec:** none — this plan is the design record. Land `docs/adr/0012-*.md` (Task 4) as the
frozen decision.

## Global Constraints

- `confy-core` stays **filesystem-free** (`tests/no_fs_gate.rs`) — no `std::fs`/`env`/`process`.
- No new dependency. Date arithmetic reuses `add_picker.rs`'s existing
  `civil_from_days`/`now_utc_parts` (public-domain Hinnant algorithm, already wasm-safe via the
  `#[cfg(target_arch = "wasm32")]` `js_sys::Date::now()` fork).
- `Mutation::ConvertKind`'s invariant is **unchanged**: "another notation of the *same* kind".
  Nothing in this plan adds a cross-type case to `convert_scalar`.
- Every user-visible string goes through `tr`/`tr_args` with keys in **both** `i18n/en.json`
  and `i18n/zh-TW.json` (`i18n/en.json` is canonical/fallback).
- JSON and YAML have no datetime type — every behavior here must be TOML-only and must not
  change `kind_options`/`add_picker_options` for the other two backends.
- Datetime seeds/fills use the clock's **UTC** instant, never a host-local timezone
  (`add_picker.rs:455-457` states this rule; keep it).

---

### Task 1: `session/datetime.rs` — parse, fill, render TOML datetime literals

**Files:**
- Create: `crates/confy-core/src/session/datetime.rs`
- Modify: `crates/confy-core/src/session/mod.rs` (add `mod datetime;`)
- Modify: `crates/confy-core/src/session/add_picker.rs:477-521` (make `now_utc_parts` and
  `civil_from_days` visible to the sibling module)
- Test: inline `#[cfg(test)] mod tests` in the new file

**Interfaces:**
- Consumes: `add_picker::now_utc_parts() -> (i64, u32, u32, u32, u32, u32)`.
- Produces:
  - `pub(crate) enum DtKind { OffsetDatetime, LocalDatetime, LocalDate, LocalTime }`
  - `pub(crate) struct DtParts { pub date: Option<(i64, u32, u32)>, pub time: Option<(u32, u32, u32)>, pub frac: Option<String>, pub offset: Option<String> }`
  - `pub(crate) fn parse_toml_datetime(repr: &str) -> Option<DtParts>`
  - `pub(crate) fn retype(parts: &DtParts, to: DtKind) -> (String, Vec<Loss>)`
  - `pub(crate) enum Loss { DroppedTime, DroppedDate, DroppedOffset, FilledDate, FilledTime, FilledOffset }`

- [ ] **Step 1: make the UTC helpers reachable from a sibling module**

In `crates/confy-core/src/session/add_picker.rs`, change the two free functions' visibility
(leave their bodies and doc comments untouched):

```rust
pub(crate) fn now_utc_parts() -> (i64, u32, u32, u32, u32, u32) {
```
```rust
pub(crate) fn civil_from_days(z: i64) -> (i64, u32, u32) {
```

- [ ] **Step 2: write the failing tests**

Create `crates/confy-core/src/session/datetime.rs` with only the test module first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn parts(s: &str) -> DtParts {
        parse_toml_datetime(s).unwrap_or_else(|| panic!("did not parse: {s}"))
    }

    #[test]
    fn parses_all_four_toml_datetime_shapes() {
        let odt = parts("1979-05-27T07:32:00Z");
        assert_eq!(odt.date, Some((1979, 5, 27)));
        assert_eq!(odt.time, Some((7, 32, 0)));
        assert_eq!(odt.offset.as_deref(), Some("Z"));

        let odt2 = parts("1979-05-27T00:32:00.999-07:00");
        assert_eq!(odt2.frac.as_deref(), Some(".999"));
        assert_eq!(odt2.offset.as_deref(), Some("-07:00"));

        // TOML permits a space separator instead of `T`.
        let ldt = parts("1979-05-27 07:32:00");
        assert_eq!(ldt.date, Some((1979, 5, 27)));
        assert_eq!(ldt.time, Some((7, 32, 0)));
        assert_eq!(ldt.offset, None);

        let ld = parts("1979-05-27");
        assert_eq!(ld.date, Some((1979, 5, 27)));
        assert_eq!(ld.time, None);

        let lt = parts("07:32:00");
        assert_eq!(lt.date, None);
        assert_eq!(lt.time, Some((7, 32, 0)));
    }

    #[test]
    fn rejects_a_non_datetime_repr() {
        assert!(parse_toml_datetime("\"hi\"").is_none());
        assert!(parse_toml_datetime("42").is_none());
    }

    #[test]
    fn retype_drops_the_components_the_target_cannot_hold() {
        let odt = parts("1979-05-27T07:32:00.5Z");

        let (s, loss) = retype(&odt, DtKind::LocalDatetime);
        assert_eq!(s, "1979-05-27T07:32:00.5");
        assert_eq!(loss, vec![Loss::DroppedOffset]);

        let (s, loss) = retype(&odt, DtKind::LocalDate);
        assert_eq!(s, "1979-05-27");
        assert_eq!(loss, vec![Loss::DroppedTime, Loss::DroppedOffset]);

        let (s, loss) = retype(&odt, DtKind::LocalTime);
        assert_eq!(s, "07:32:00.5");
        assert_eq!(loss, vec![Loss::DroppedDate, Loss::DroppedOffset]);
    }

    #[test]
    fn retype_fills_a_missing_component_and_reports_it() {
        // A local date has no time: widening to a datetime fills 00:00:00,
        // a fixed midnight rather than the clock, so the *date* the user
        // can see is preserved exactly and the result is reproducible.
        let ld = parts("1979-05-27");
        let (s, loss) = retype(&ld, DtKind::LocalDatetime);
        assert_eq!(s, "1979-05-27T00:00:00");
        assert_eq!(loss, vec![Loss::FilledTime]);

        let (s, loss) = retype(&ld, DtKind::OffsetDatetime);
        assert_eq!(s, "1979-05-27T00:00:00Z");
        assert_eq!(loss, vec![Loss::FilledTime, Loss::FilledOffset]);

        // A local time has no date: it must come from the clock (there is no
        // meaningful fixed default), so assert the shape, not the value.
        let lt = parts("07:32:00");
        let (s, loss) = retype(&lt, DtKind::LocalDate);
        assert_eq!(s.len(), 10, "YYYY-MM-DD, got {s}");
        assert_eq!(loss, vec![Loss::FilledDate]);

        let (s, loss) = retype(&lt, DtKind::LocalDatetime);
        assert!(s.ends_with("T07:32:00"), "kept the time: {s}");
        assert_eq!(loss, vec![Loss::FilledDate]);
    }

    #[test]
    fn retype_to_the_same_kind_is_lossless_and_identical() {
        for src in [
            "1979-05-27T07:32:00Z",
            "1979-05-27T07:32:00",
            "1979-05-27",
            "07:32:00",
        ] {
            let p = parts(src);
            let (s, loss) = retype(&p, kind_of(&p));
            assert_eq!(s, src, "round-trip {src}");
            assert!(loss.is_empty(), "{src} reported {loss:?}");
        }
    }
}
```

- [ ] **Step 3: run the tests to verify they fail**

Run: `cargo test -p confy-core --lib session::datetime`
Expected: compile error — `cannot find function parse_toml_datetime`.

- [ ] **Step 4: write the implementation**

Prepend to `crates/confy-core/src/session/datetime.rs` (above the test module):

```rust
//! TOML datetime component surgery for the `K` datetime switch: decompose a
//! datetime literal, re-render it as any of TOML's four datetime types, and
//! report what was dropped or auto-filled on the way. Pure — no document, no
//! session, no filesystem; the only impurity is the UTC clock read that fills
//! a genuinely absent date (`add_picker::now_utc_parts`, the same reader the
//! `a` picker's datetime seeds already use).
//!
//! Why this is not `Mutation::ConvertKind`: a `[D:ldat]` -> `[D:odt]` switch
//! changes the node's *type*, not its notation, so it commits as a plain value
//! `Replace` and inherits the `PromptKind::TypeChange` gate. See ADR 0012.

use super::add_picker::now_utc_parts;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DtKind {
    OffsetDatetime,
    LocalDatetime,
    LocalDate,
    LocalTime,
}

/// What a `retype` cost: a component the target cannot hold was dropped, or a
/// component the target requires was absent and had to be filled in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Loss {
    DroppedDate,
    DroppedTime,
    DroppedOffset,
    FilledDate,
    FilledTime,
    FilledOffset,
}

/// A TOML datetime literal taken apart. `frac` keeps the fractional-second
/// text **including** its leading `.` so an authored `.5` is never re-rendered
/// as `.500`; `offset` keeps `Z`/`+09:00` verbatim for the same reason.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct DtParts {
    pub date: Option<(i64, u32, u32)>,
    pub time: Option<(u32, u32, u32)>,
    pub frac: Option<String>,
    pub offset: Option<String>,
}

/// The datetime type `parts` currently *is* — used to exclude the current type
/// from the picker's option list (mirroring `kind_options`' notation filter).
pub(crate) fn kind_of(parts: &DtParts) -> DtKind {
    match (parts.date.is_some(), parts.time.is_some(), parts.offset.is_some()) {
        (true, true, true) => DtKind::OffsetDatetime,
        (true, true, false) => DtKind::LocalDatetime,
        (true, false, _) => DtKind::LocalDate,
        _ => DtKind::LocalTime,
    }
}

/// Decompose a TOML datetime literal. `None` when `repr` is not one of TOML's
/// four datetime shapes — the caller (`open_kind_switch`) has already narrowed
/// to a datetime `ScalarType`, so this is a defensive parse, not a classifier.
pub(crate) fn parse_toml_datetime(repr: &str) -> Option<DtParts> {
    let s = repr.trim();
    // Split date from time on TOML's two legal separators.
    let (date_str, rest) = match s.split_once(['T', 't', ' ']) {
        Some((d, r)) => (Some(d), Some(r)),
        // No separator: either a bare date or a bare time.
        None if s.contains(':') => (None, Some(s)),
        None => (Some(s), None),
    };

    let mut out = DtParts::default();
    if let Some(d) = date_str {
        let mut it = d.split('-');
        let y: i64 = it.next()?.parse().ok()?;
        let mo: u32 = it.next()?.parse().ok()?;
        let da: u32 = it.next()?.parse().ok()?;
        if it.next().is_some() || !(1..=12).contains(&mo) || !(1..=31).contains(&da) {
            return None;
        }
        out.date = Some((y, mo, da));
    }
    if let Some(r) = rest {
        // Peel the offset off the right first: `Z`, `+HH:MM` or `-HH:MM`.
        let (time_str, offset) = if let Some(t) = r.strip_suffix(['Z', 'z']) {
            (t, Some("Z".to_string()))
        } else if r.len() >= 6 {
            let (t, off) = r.split_at(r.len() - 6);
            if off.starts_with('+') || off.starts_with('-') {
                (t, Some(off.to_string()))
            } else {
                (r, None)
            }
        } else {
            (r, None)
        };
        out.offset = offset;
        let (hms, frac) = match time_str.split_once('.') {
            Some((h, f)) => (h, Some(format!(".{f}"))),
            None => (time_str, None),
        };
        let mut it = hms.split(':');
        let h: u32 = it.next()?.parse().ok()?;
        let mi: u32 = it.next()?.parse().ok()?;
        // TOML permits `HH:MM` with no seconds.
        let se: u32 = match it.next() {
            Some(x) => x.parse().ok()?,
            None => 0,
        };
        if it.next().is_some() || h > 23 || mi > 59 || se > 60 {
            return None;
        }
        out.time = Some((h, mi, se));
        out.frac = frac;
    }
    if out.date.is_none() && out.time.is_none() {
        return None;
    }
    Some(out)
}

/// Re-render `parts` as `to`, filling anything the target requires and
/// dropping anything it cannot hold. Returns the literal plus the ordered
/// loss/fill report the picker label discloses.
///
/// Fill policy: a missing **time** becomes `00:00:00` (a fixed midnight — the
/// authored date stays exactly what the user sees, and the result is
/// reproducible), a missing **offset** becomes `Z` (UTC, matching the `a`
/// picker's seeds), and a missing **date** — which has no meaningful fixed
/// default — comes from the UTC clock.
pub(crate) fn retype(parts: &DtParts, to: DtKind) -> (String, Vec<Loss>) {
    let mut loss: Vec<Loss> = Vec::new();
    let wants_date = matches!(
        to,
        DtKind::OffsetDatetime | DtKind::LocalDatetime | DtKind::LocalDate
    );
    let wants_time = matches!(
        to,
        DtKind::OffsetDatetime | DtKind::LocalDatetime | DtKind::LocalTime
    );
    let wants_offset = to == DtKind::OffsetDatetime;

    let date = match (wants_date, parts.date) {
        (true, Some(d)) => Some(d),
        (true, None) => {
            loss.push(Loss::FilledDate);
            let (y, mo, d, ..) = now_utc_parts();
            Some((y, mo, d))
        }
        (false, Some(_)) => {
            loss.push(Loss::DroppedDate);
            None
        }
        (false, None) => None,
    };
    let time = match (wants_time, parts.time) {
        (true, Some(t)) => Some(t),
        (true, None) => {
            loss.push(Loss::FilledTime);
            Some((0, 0, 0))
        }
        (false, Some(_)) => {
            loss.push(Loss::DroppedTime);
            None
        }
        (false, None) => None,
    };
    let offset = match (wants_offset, parts.offset.clone()) {
        (true, Some(o)) => Some(o),
        (true, None) => {
            loss.push(Loss::FilledOffset);
            Some("Z".to_string())
        }
        (false, Some(_)) => {
            loss.push(Loss::DroppedOffset);
            None
        }
        (false, None) => None,
    };
    // The fractional second rides with the time and is dropped with it.
    let frac = if time.is_some() {
        parts.frac.clone().unwrap_or_default()
    } else {
        String::new()
    };

    let mut s = String::new();
    if let Some((y, mo, d)) = date {
        s.push_str(&format!("{y:04}-{mo:02}-{d:02}"));
    }
    if let Some((h, mi, se)) = time {
        if date.is_some() {
            s.push('T');
        }
        s.push_str(&format!("{h:02}:{mi:02}:{se:02}{frac}"));
    }
    if let Some(o) = offset {
        s.push_str(&o);
    }
    (s, loss)
}
```

Register the module in `crates/confy-core/src/session/mod.rs` beside the existing
`mod add_picker;` line, in alphabetical position:

```rust
mod datetime;
```

- [ ] **Step 5: run the tests to verify they pass**

Run: `cargo test -p confy-core --lib session::datetime`
Expected: 5 passed.

Note the `retype_to_the_same_kind_is_lossless_and_identical` case
`"1979-05-27T07:32:00Z"` — it round-trips only because `parse_toml_datetime` keeps `Z` and
`frac` verbatim. If it fails, the bug is in the verbatim-preservation rule, not the test.

- [ ] **Step 6: commit**

```bash
git add crates/confy-core/src/session/datetime.rs crates/confy-core/src/session/mod.rs crates/confy-core/src/session/add_picker.rs
git commit -m "feat(core): datetime component surgery for the K datetime switch

parse_toml_datetime / retype decompose a TOML datetime literal and re-render
it as any of the four datetime types, reporting dropped and auto-filled
components. Pure module; reuses add_picker's existing UTC clock helpers
(now pub(crate)) rather than adding a date crate."
```

---

### Task 2: `K` on a datetime opens the picker

**Files:**
- Modify: `crates/confy-core/src/session/session.rs:1080-1105` (`open_kind_switch`)
- Modify: `i18n/en.json`, `i18n/zh-TW.json`
- Test: `crates/confy-core/tests/session_headless.rs`

**Interfaces:**
- Consumes: Task 1's `parse_toml_datetime`/`retype`/`kind_of`/`DtKind`/`Loss`;
  the existing `Mode::SchemaEnum(SchemaEnumState)` (`state.rs:192-201`) and
  `Session::schema_enum_commit` (`inline_edit.rs:1010`).
- Produces: nothing new for later tasks.

- [ ] **Step 1: add the i18n keys**

Add to `i18n/en.json` (keep the file's existing `core.*` grouping and trailing-comma style):

```json
  "core.dt.target.offset-datetime": "offset datetime",
  "core.dt.target.local-datetime": "local datetime",
  "core.dt.target.local-date": "local date",
  "core.dt.target.local-time": "local time",
  "core.dt.loss.dropped-date": "drops the date",
  "core.dt.loss.dropped-time": "drops the time",
  "core.dt.loss.dropped-offset": "drops the offset",
  "core.dt.loss.filled-date": "fills today's date",
  "core.dt.loss.filled-time": "fills 00:00:00",
  "core.dt.loss.filled-offset": "fills Z",
```

and the same keys in `i18n/zh-TW.json`:

```json
  "core.dt.target.offset-datetime": "帶時區日期時間",
  "core.dt.target.local-datetime": "本地日期時間",
  "core.dt.target.local-date": "本地日期",
  "core.dt.target.local-time": "本地時間",
  "core.dt.loss.dropped-date": "丟失日期",
  "core.dt.loss.dropped-time": "丟失時間",
  "core.dt.loss.dropped-offset": "丟失時區",
  "core.dt.loss.filled-date": "補上今日日期",
  "core.dt.loss.filled-time": "補上 00:00:00",
  "core.dt.loss.filled-offset": "補上 Z",
```

Verify both files still parse:
`python3 -c "import json;[json.load(open(f)) for f in ['i18n/en.json','i18n/zh-TW.json']];print('ok')"`

- [ ] **Step 2: write the failing test**

Append to `crates/confy-core/tests/session_headless.rs`:

```rust
/// `K` on a TOML datetime opens the SchemaEnum *picker* (not `Mode::KindSwitch`),
/// listing the three other datetime types with their rendered literal and the
/// loss/fill disclosure. Committing goes through `edit_commit`, so the existing
/// TypeChange prompt gates it. See ADR 0012.
#[test]
fn kind_switch_on_a_datetime_offers_the_other_three_types() {
    let mut s = toml_session("odt = 1979-05-27T07:32:00Z\n");
    s.dispatch(Intent::CursorDown);
    s.dispatch(Intent::OpenKindSwitch);
    let labels: Vec<String> = match s.mode_view() {
        ModeView::SchemaEnum { options, from_schema, .. } => {
            assert!(!from_schema, "a datetime picker is not schema-driven");
            options.iter().map(|(l, _)| l.clone()).collect()
        }
        other => panic!("expected the SchemaEnum picker, got {other:?}"),
    };
    assert_eq!(labels.len(), 3, "the current type is excluded: {labels:?}");
    let joined = labels.join(" | ");
    assert!(joined.contains("local datetime  1979-05-27T07:32:00"), "{joined}");
    assert!(joined.contains("drops the offset"), "{joined}");
    assert!(joined.contains("local date  1979-05-27"), "{joined}");
    assert!(joined.contains("local time  07:32:00"), "{joined}");
}

/// Picking a lossy target raises the ordinary TypeChange confirmation; `y`
/// commits it and the node's projected kind really changes.
#[test]
fn datetime_switch_confirms_the_type_change_and_applies_it() {
    let mut s = toml_session("odt = 1979-05-27T07:32:00Z\n");
    s.dispatch(Intent::CursorDown);
    s.dispatch(Intent::OpenKindSwitch);
    // options are [local datetime, local date, local time]; pick local date.
    s.dispatch(Intent::SchemaEnumMove(1));
    s.dispatch(Intent::SchemaEnumCommit);
    assert!(
        matches!(s.mode_view(), ModeView::Prompt { .. }),
        "a datetime retype must confirm, got {:?}",
        s.mode_view()
    );
    s.dispatch(Intent::PromptKey('y'));
    assert_eq!(s.serialize().unwrap(), "odt = 1979-05-27\n");
    assert!(matches!(
        s.tree.node_at(&vec![Seg::Key("odt".into())]).unwrap().kind,
        confy_core::model::node::NodeKind::Scalar(
            confy_core::model::node::ScalarType::LocalDate
        )
    ));
}

/// A widening switch fills the missing component instead of failing, and the
/// picker said so up front.
#[test]
fn datetime_switch_widens_a_local_date_by_filling_midnight() {
    let mut s = toml_session("d = 1979-05-27\n");
    s.dispatch(Intent::CursorDown);
    s.dispatch(Intent::OpenKindSwitch);
    let ModeView::SchemaEnum { options, .. } = s.mode_view() else {
        panic!("expected the picker");
    };
    let (label, value) = options
        .iter()
        .find(|(l, _)| l.starts_with("local datetime"))
        .expect("local datetime option")
        .clone();
    assert!(label.contains("fills 00:00:00"), "{label}");
    assert_eq!(value, "1979-05-27T00:00:00");
}

/// Non-TOML backends have no datetime type, so nothing here may leak into
/// them: `K` on a YAML date-looking scalar is still a plain string with no
/// convertible datetime notations.
#[test]
fn yaml_date_looking_scalar_gets_no_datetime_picker() {
    let mut s = yaml_session("d: 1979-05-27\n");
    s.dispatch(Intent::CursorDown);
    s.dispatch(Intent::OpenKindSwitch);
    match s.mode_view() {
        ModeView::KindSwitch { options, .. } => {
            let joined = options.iter().map(|o| o.label.clone()).collect::<Vec<_>>().join("|");
            assert!(!joined.contains("datetime"), "{joined}");
        }
        // A string with no alternative notation legitimately reports
        // "unsupported" and stays in Normal.
        ModeView::Normal => {}
        other => panic!("unexpected {other:?}"),
    }
}
```

Before running, confirm the exact `Intent` and `ModeView` variant names in scope —
`crates/confy-core/src/session/intent.rs` (`OpenKindSwitch`, `SchemaEnumMove`,
`SchemaEnumCommit`, `PromptKey`) and `view.rs`'s `ModeView::SchemaEnum`/`KindSwitch` field
names. Adjust the test's destructuring to match the real shapes rather than editing the
source to match the test.

- [ ] **Step 3: run the tests to verify they fail**

Run: `cargo test -p confy-core --test session_headless datetime`
Expected: FAIL — `kind_switch_on_a_datetime_offers_the_other_three_types` panics with
`expected the SchemaEnum picker, got Normal` (today `kind_options` returns an empty list for a
datetime and `open_kind_switch` sets the `core.kind-switch.unsupported` notice).

- [ ] **Step 4: implement the divert in `open_kind_switch`**

In `crates/confy-core/src/session/session.rs`, insert between the `let Some(doc)` binding
(ends line 1094) and `let options = doc.kind_options(&path);` (line 1095):

```rust
        // A TOML datetime's four types are mutually convertible, but that is a
        // *type* change, not a notation change — so it does not go through
        // `kind_options`/`Mutation::ConvertKind` (whose invariant is
        // same-kind-only). `K` instead opens the value picker, whose commit
        // path (`schema_enum_commit` -> `edit_commit`) already gates a type
        // change behind `PromptKind::TypeChange`. ADR 0012.
        if let Some(st) = self.datetime_picker_state(&path) {
            self.mode = Mode::SchemaEnum(st);
            return;
        }
```

Then add the builder as a new method on the same `impl Session` block, immediately after
`open_kind_switch` (after line 1105):

```rust
    /// The `K` datetime picker's state for `path`, or `None` when `path` is not
    /// a TOML datetime scalar whose literal parses. Options are the **other**
    /// three datetime types (the current one is excluded, mirroring
    /// `kind_options`' notation filter), each labelled
    /// `"<type>  <resulting literal>  (<loss/fill>, …)"` so the cost is
    /// disclosed *before* the commit prompt, and valued with the literal itself
    /// so the ordinary value-`Replace` path can apply it verbatim.
    fn datetime_picker_state(&self, path: &Path) -> Option<SchemaEnumState> {
        use super::datetime::{kind_of, parse_toml_datetime, retype, DtKind, Loss};
        let node = self.tree.node_at(path)?;
        if !matches!(
            node.kind,
            NodeKind::Scalar(
                ScalarType::OffsetDatetime
                    | ScalarType::LocalDatetime
                    | ScalarType::LocalDate
                    | ScalarType::LocalTime
            )
        ) {
            return None;
        }
        let parts = parse_toml_datetime(node.value.as_deref()?)?;
        let current = kind_of(&parts);
        let lang = self.lang;
        let type_key = |k: DtKind| match k {
            DtKind::OffsetDatetime => "core.dt.target.offset-datetime",
            DtKind::LocalDatetime => "core.dt.target.local-datetime",
            DtKind::LocalDate => "core.dt.target.local-date",
            DtKind::LocalTime => "core.dt.target.local-time",
        };
        let loss_key = |l: Loss| match l {
            Loss::DroppedDate => "core.dt.loss.dropped-date",
            Loss::DroppedTime => "core.dt.loss.dropped-time",
            Loss::DroppedOffset => "core.dt.loss.dropped-offset",
            Loss::FilledDate => "core.dt.loss.filled-date",
            Loss::FilledTime => "core.dt.loss.filled-time",
            Loss::FilledOffset => "core.dt.loss.filled-offset",
        };
        let options: Vec<(String, String)> = [
            DtKind::OffsetDatetime,
            DtKind::LocalDatetime,
            DtKind::LocalDate,
            DtKind::LocalTime,
        ]
        .into_iter()
        .filter(|k| *k != current)
        .map(|k| {
            let (lit, loss) = retype(&parts, k);
            let mut label = format!("{}  {}", tr(lang, type_key(k)), lit);
            if !loss.is_empty() {
                let notes: Vec<&str> = loss.into_iter().map(|l| tr(lang, loss_key(l))).collect();
                label.push_str(&format!("  ({})", notes.join(", ")));
            }
            (label, lit)
        })
        .collect();
        if options.is_empty() {
            return None;
        }
        Some(SchemaEnumState {
            path: path.clone(),
            key: node.key.clone(),
            is_element: matches!(path.last(), Some(Seg::Index(_))),
            created_on_add: false,
            options,
            cursor: 0,
            from_schema: false,
        })
    }
```

If `tr`, `SchemaEnumState`, `ScalarType`, `NodeKind`, `Path` or `Seg` are not already imported
in `session.rs`, add them to the existing `use` block at the top of the file rather than
fully-qualifying inline.

- [ ] **Step 5: run the tests to verify they pass**

Run: `cargo test -p confy-core --test session_headless datetime`
Expected: 4 passed.
Then `cargo test -p confy-core` — expected: all green. In particular
`crates/confy-tui/src/tui/tests.rs` and the existing kind-switch tests must be unaffected,
because `kind_options` is untouched.

- [ ] **Step 6: commit**

```bash
git add crates/confy-core/src/session/session.rs crates/confy-core/tests/session_headless.rs i18n/en.json i18n/zh-TW.json
git commit -m "feat(kind-switch): K on a TOML datetime picks among the four datetime types

A datetime cross-type switch is a value Replace, not a ConvertKind: K diverts
to the existing SchemaEnum picker (from_schema: false), whose options carry the
fully-rendered target literal and disclose every dropped/auto-filled component
in the label. schema_enum_commit already routes through edit_commit, so the
existing PromptKind::TypeChange gate confirms the change with no new prompt
plumbing. ConvertKind's same-kind invariant is untouched, and JSON/YAML are
unaffected. ADR 0012."
```

---

### Task 3: one consolidated datetime row in the `a` Add-type picker

**Files:**
- Modify: `crates/confy-core/src/session/add_picker.rs:162-184`
- Modify: `i18n/en.json`, `i18n/zh-TW.json`
- Test: `crates/confy-core/tests/session_headless.rs`

**Interfaces:**
- Consumes: `scalar_seed_literal` (`add_picker.rs:441`), unchanged.
- Produces: nothing.

- [ ] **Step 1: add the i18n key**

`i18n/en.json`:
```json
  "core.add.type.datetime": "Datetime",
```
`i18n/zh-TW.json`:
```json
  "core.add.type.datetime": "日期時間",
```

Leave the four existing `core.add.type.offset-datetime` / `core.add.type.local-datetime` /
`core.add.type.local-date` / `core.add.type.local-time` keys in place for now — Step 5 removes
them once the code no longer references them. (Task 2's picker uses its own
`core.dt.target.*` keys, which are worded as lowercase inline phrases rather than the Add
picker's capitalized menu labels, so the two sets are deliberately not shared.)

- [ ] **Step 2: write the failing test**

Append to `crates/confy-core/tests/session_headless.rs`:

```rust
/// The `a` picker offers **one** datetime row (seeded as a full offset
/// datetime, the only one of the four that loses nothing on a later `K`
/// switch), not four. The other three are reachable from `K`.
#[test]
fn add_picker_offers_a_single_datetime_row() {
    let mut s = toml_session("a = 1\n");
    s.dispatch(Intent::CursorDown);
    s.dispatch(Intent::AddNode);
    let ModeView::AddPicker { options, .. } = s.mode_view() else {
        panic!("expected the add picker, got {:?}", s.mode_view());
    };
    let dt: Vec<&String> = options
        .iter()
        .map(|o| &o.label)
        .filter(|l| l.to_lowercase().contains("datetime") || l.contains("日期"))
        .collect();
    assert_eq!(dt.len(), 1, "expected one datetime row, got {dt:?}");
}
```

Confirm `ModeView::AddPicker`'s real field/option shape in
`crates/confy-core/src/session/view.rs` and match it; do not reshape the view to fit the test.

- [ ] **Step 3: run it to verify it fails**

Run: `cargo test -p confy-core --test session_headless add_picker_offers_a_single`
Expected: FAIL — `expected one datetime row, got ["Offset datetime", "Local datetime", "Local date", "Local time"]`.

- [ ] **Step 4: implement**

Replace `add_picker.rs` lines 163-184 (the `DocFormat::Toml =>` arm's body) with:

```rust
            DocFormat::Toml => {
                // One row, not four: an offset datetime is the widest of TOML's
                // four datetime types, so it is the only seed a later `K`
                // switch can narrow without having filled anything in first.
                // The other three are reachable from `K` (ADR 0012).
                push(
                    &mut out,
                    "core.add.type.datetime",
                    AddKind::Scalar(ScalarType::OffsetDatetime),
                );
            }
```

- [ ] **Step 5: run the tests to verify they pass**

Run: `cargo test -p confy-core --test session_headless add_picker`
Expected: pass. Then `cargo test -p confy-core` — if an existing add-picker test asserts a
four-row datetime list or a total option count, update that assertion (the list genuinely
changed) rather than reverting this.

Then remove the four now-dead i18n keys:
```bash
grep -rn "core.add.type.offset-datetime\|core.add.type.local-datetime\|core.add.type.local-date\|core.add.type.local-time" crates web editors i18n
```
Delete them from both catalogs only if the only remaining hits are the catalog lines themselves.

- [ ] **Step 6: commit**

```bash
git add crates/confy-core/src/session/add_picker.rs crates/confy-core/tests/session_headless.rs i18n/en.json i18n/zh-TW.json
git commit -m "feat(add-picker): one datetime row for TOML instead of four

Seeds a full offset datetime (the widest of the four, so a later K switch only
ever narrows). The other three types are reachable from K per ADR 0012."
```

---

### Task 4: ADR 0012, reference docs, CHANGELOG, and the real-binary check

**Files:**
- Create: `docs/adr/0012-datetime-cross-type-switch-is-a-value-replace.md`
- Modify: `docs/adr/README.md` (index row + the superseded-notes paragraph)
- Modify: `docs/reference/TUI.md` (the `K` kind-switch material), `docs/reference/CONTEXT.md`
  (the *Kind switch (`K`) rules* section), `CLAUDE.md` (the `K` sentence under
  **Kind switch (`K`)**)
- Modify: `CHANGELOG.md` (new `### Unreleased Update - <date>` block)

- [ ] **Step 1: write ADR 0012**

Content must state, at minimum:
- **Context.** `Mutation::ConvertKind`'s documented invariant (`document.rs:278-285`) is
  "another notation of the *same* kind". TOML's four datetime types are four *types*
  (`ScalarType::{OffsetDatetime, LocalDatetime, LocalDate, LocalTime}`), so a switch among
  them is a type change; `kind_options` therefore returned an empty list for them
  (`cst_doc.rs:89-91`) and `K` reported `core.kind-switch.unsupported`.
- **Decision.** `K` on a datetime opens `Mode::SchemaEnum` (`from_schema: false`) rather than
  `Mode::KindSwitch`; the picked option's value is a pre-rendered literal committed as an
  ordinary value `Replace`. Rejected alternative: adding four `KindTarget::Datetime*` variants
  and a cross-type arm to `convert_scalar` — that would have broken the same-kind invariant,
  required new confirm plumbing in `kind_switch_commit` (which applies directly and has no
  prompt gate), and grown the serde wire contract (`KindTarget` is in `web/types.ts`) plus a
  `convert_scalar` arm in three backends for a TOML-only feature.
- **Consequences.** Zero new `Mutation`/`KindTarget`/`Mode`/`PromptKind`/wire-contract/host
  changes. The confirmation the user asked for is the *existing* `PromptKind::TypeChange`; the
  loss/fill disclosure is in the picker label, i.e. visible *before* the prompt. The cost is
  that `K` now has two possible popups depending on the node — documented in TUI.md.
- **Fill policy** (and why): missing time → fixed `00:00:00`; missing offset → `Z`; missing
  date → the UTC clock, since it has no meaningful fixed default.

- [ ] **Step 2: index it**

Add one row to `docs/adr/README.md`'s table:

```markdown
| [0012](0012-datetime-cross-type-switch-is-a-value-replace.md) | A TOML datetime cross-type switch is a value `Replace` behind the `K` key, not a `ConvertKind` | Implemented (<date>) |
```

- [ ] **Step 3: update the reference docs**

- `docs/reference/TUI.md`: in the `K` material, add that on a TOML datetime `K` opens the
  **value picker** (the `true`/`false` picker's widget) listing the other three datetime types
  with their resulting literal and the dropped/filled components, and that `Enter` then hits
  the ordinary TypeChange confirmation.
- `docs/reference/CONTEXT.md` *Kind switch (`K`) rules*: record that datetimes are the one `K`
  case that is not a `ConvertKind`, pointing at ADR 0012.
- `CLAUDE.md`, under **Kind switch (`K`)**: append one sentence — a TOML datetime is the
  exception, routed to the value picker as a `Replace` (ADR 0012).
- `docs/reference/TUI.md` / wherever the `a` picker's TOML type list is enumerated: four
  datetime rows → one.

- [ ] **Step 4: CHANGELOG**

Add a `### Unreleased Update - <today>` block under `## [Unreleased]` with an **Added**
subsection covering both the `K` datetime switch (including the fill policy and the ADR
reference) and the consolidated `a` row, in the file's existing prose style.

- [ ] **Step 5: full verification**

```bash
cargo fmt && cargo fmt --check && cargo clippy --all-targets 2>&1 | grep -E "^(error|warning)"
cargo test
cd web && npm run typecheck && npm test
```
Expected: clippy/fmt clean, all Rust tests pass, web unchanged and green (no host file was
touched — if `npm run typecheck` fails, something leaked into the wire contract and Task 2's
design was not followed).

- [ ] **Step 6: real-binary check (mandatory — green unit tests are not a fix)**

```bash
mkdir -p docs/tmp/claude-scratch
printf 'odt = 1979-05-27T07:32:00Z\nld = 1979-05-27\nlt = 07:32:00\n' > docs/tmp/claude-scratch/dt.toml
cargo build -p confy-tui
```
Then drive the real TUI (`hub` `op:"start"` with `--lang en`, never a bare `bash` background
job) and confirm by reading the rendered frames:
1. cursor on `odt`, press `K` → a 3-row popup: `local datetime  1979-05-27T07:32:00  (drops the offset)`, `local date  1979-05-27  (drops the time, drops the offset)`, `local time  07:32:00  (drops the date, drops the offset)`.
2. pick `local date`, `Enter` → the TypeChange prompt appears; `y` → the row's KIND column
   becomes `[D:ldat]` and the VALUE becomes `1979-05-27`.
3. cursor on `lt` (a bare local time), press `K` → the `local date` row's label says
   `fills today's date` and shows **today's** UTC date, not `1970-01-01`.
4. press `a` → exactly one datetime row in the Add-type picker.
5. `z` (undo) restores the original literal byte-for-byte (`:w` then `git diff` on the
   fixture, or re-read the file).

- [ ] **Step 7: commit**

```bash
git add docs CHANGELOG.md CLAUDE.md
git commit -m "docs: ADR 0012 + K datetime switch reference/changelog updates"
```

---

## Self-review notes

- **Coverage.** The original ask had four parts: (a) the four datetimes become mutually
  convertible — Task 2; (b) a confirm popup on information loss — the existing
  `PromptKind::TypeChange`, asserted in
  `datetime_switch_confirms_the_type_change_and_applies_it`, with the loss itemized in the
  picker label one step earlier; (c) a confirm popup when a component must be filled — same
  gate, with the fill named in the label
  (`datetime_switch_widens_a_local_date_by_filling_midnight`), and the "current vs. specific
  datetime" question resolved by the documented fill policy rather than a second prompt;
  (d) one consolidated `a` entry — Task 3.
- **The `K`-vs-`e` split stays intact.** `e` on a datetime still opens the inline text editor
  (free-form literal editing); only `K` opens the picker. Nothing in this plan touches
  `edit_target_kind`.
- **Deliberate non-goal.** No calendar/spinner widget. The picker lists four fully-rendered
  literals; changing the *value* remains `e`'s job.
