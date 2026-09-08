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
    match (
        parts.date.is_some(),
        parts.time.is_some(),
        parts.offset.is_some(),
    ) {
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

/// Catalog key for a datetime type's human name (the picker's option name
/// column, and the confirm prompt when it needs to name a type).
pub(crate) fn type_key(k: DtKind) -> &'static str {
    match k {
        DtKind::OffsetDatetime => "core.dt.target.offset-datetime",
        DtKind::LocalDatetime => "core.dt.target.local-datetime",
        DtKind::LocalDate => "core.dt.target.local-date",
        DtKind::LocalTime => "core.dt.target.local-time",
    }
}

/// The short type suffix the TUI's KIND column uses (`[D:odt ]` -> `odt`), and
/// the picker option's second column as `[D:odt]`. Kept next to `DtKind` so
/// the tag and the retype logic can't drift; `status_fmt::datetime_note`
/// serves the same four strings off a `NodeKind` for the row badge.
pub(crate) fn short_tag(k: DtKind) -> &'static str {
    match k {
        DtKind::OffsetDatetime => "odt",
        DtKind::LocalDatetime => "ldt",
        DtKind::LocalDate => "ldat",
        DtKind::LocalTime => "ltim",
    }
}

/// Catalog key for one loss/fill item.
fn loss_key(l: Loss) -> &'static str {
    match l {
        Loss::DroppedDate => "core.dt.loss.dropped-date",
        Loss::DroppedTime => "core.dt.loss.dropped-time",
        Loss::DroppedOffset => "core.dt.loss.dropped-offset",
        Loss::FilledDate => "core.dt.loss.filled-date",
        Loss::FilledTime => "core.dt.loss.filled-time",
        Loss::FilledOffset => "core.dt.loss.filled-offset",
    }
}

/// What changing a datetime node's value from `old` to `new` does, as
/// translated prose - the disclosure the `PromptKind::TypeChange` confirm
/// carries. Leads with a **preview of the rewritten literal**
/// (`1979-05-27T07:32:00Z -> 1979-05-27`), then what that costs
/// ("drops the time, drops the offset"). `None` when either side is not a
/// datetime literal (an ordinary type change has nothing datetime-specific to
/// say).
///
/// The preview is why the picker's own rows no longer carry the resulting
/// literal: the confirm is the last moment before the value is rewritten, and
/// it names one concrete outcome instead of four hypothetical ones.
///
/// Derived from the *values*, not from how the edit was started, so a
/// hand-typed `e` that happens to retype a datetime discloses the same thing
/// the `K` picker does.
pub(crate) fn change_note(lang: super::i18n::Lang, old: &str, new: &str) -> Option<String> {
    let old_parts = parse_toml_datetime(old)?;
    let new_kind = kind_of(&parse_toml_datetime(new)?);
    let (lit, loss) = retype(&old_parts, new_kind);
    let mut notes: Vec<String> = vec![super::i18n::tr_args(
        lang,
        "core.dt.preview",
        &[old, lit.as_str()],
    )];
    notes.extend(
        loss.into_iter()
            .map(|l| super::i18n::tr(lang, loss_key(l)).to_string()),
    );
    // The separator is translated too - zh-TW enumerates with `、`, not `, `.
    Some(notes.join(super::i18n::tr(lang, "core.list.sep")))
}

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
        // Narrowing it to a local *date* also throws the authored time away,
        // and both halves of that are disclosed — a fill and a drop can occur
        // in the same switch, and the label must name each.
        let lt = parts("07:32:00");
        let (s, loss) = retype(&lt, DtKind::LocalDate);
        assert_eq!(s.len(), 10, "YYYY-MM-DD, got {s}");
        assert_eq!(loss, vec![Loss::FilledDate, Loss::DroppedTime]);

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
