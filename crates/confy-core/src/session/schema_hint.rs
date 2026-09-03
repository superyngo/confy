//! Schema-constraint numeric clamping for the nudge (`←`/`→`) shortcut:
//! `nudge_scalar` and its digit-grouping helpers — split out of `session.rs`
//! (Task 15, 2026-08-11 audit remediation).

use crate::model::node::{Format, ScalarType};

pub(crate) fn nudge_scalar(st: ScalarType, fmt: Format, repr: &str, delta: i64) -> Option<String> {
    let s = repr.trim();
    match st {
        ScalarType::Integer => {
            let had_us = s.contains('_');
            let clean = s.replace('_', "");
            let out = match fmt {
                Format::Hex => {
                    let upper = clean[2..].chars().any(|c| c.is_ascii_uppercase());
                    let n = i64::from_str_radix(&clean[2..], 16).ok()? + delta;
                    if upper {
                        format!("0x{n:X}")
                    } else {
                        format!("0x{n:x}")
                    }
                }
                Format::Octal => {
                    let n = i64::from_str_radix(&clean[2..], 8).ok()? + delta;
                    format!("0o{n:o}")
                }
                Format::Binary => {
                    let n = i64::from_str_radix(&clean[2..], 2).ok()? + delta;
                    format!("0b{n:b}")
                }
                _ => {
                    let n = clean.parse::<i64>().ok()? + delta;
                    n.to_string()
                }
            };
            Some(if had_us { regroup_int(&out, fmt) } else { out })
        }
        ScalarType::Float => {
            let had_us = s.contains('_');
            let clean = s.replace('_', "");
            if clean
                .bytes()
                .any(|b| matches!(b, b'e' | b'E') || b.is_ascii_alphabetic())
            {
                return None;
            }
            let places = clean
                .split_once('.')
                .map(|(_, frac)| frac.len())
                .unwrap_or(0);
            let val = clean.parse::<f64>().ok()?;
            let step = 10f64.powi(-(places as i32));
            let next = val + delta as f64 * step;
            let out = format!("{next:.*}", places);
            Some(if had_us { regroup_float(&out) } else { out })
        }
        _ => None,
    }
}

/// Render a schema-adjusted nudge result (`Session::schema_clamp_nudge`).
/// An integer-style repr yielding a whole number formats as an integer; a
/// float-style repr keeps at least one decimal (a bare `5` would silently
/// retype a Float node as Integer), and a `multipleOf` grid's own decimal
/// count sets the precision so a 0.1 grid can't surface float noise
/// (`0.30000000000000004`).
pub(crate) fn format_nudged(n: f64, step: Option<f64>, int_style: bool) -> String {
    if int_style && n.fract() == 0.0 {
        return format!("{}", n as i64);
    }
    let places = step
        .map(|s| format!("{s}"))
        .filter(|s| !s.contains(['e', 'E']))
        .and_then(|s| s.split_once('.').map(|(_, frac)| frac.len()));
    let out = match places {
        Some(p) => format!("{n:.*}", p),
        None => format!("{n}"),
    };
    if int_style || out.contains('.') {
        out
    } else {
        format!("{out}.0")
    }
}

/// Decode a nudgeable scalar repr to `f64` for schema stepping/clamping:
/// underscore grouping is stripped, and a non-decimal integer notation
/// (`0x`/`0o`/`0b`) is decoded **in its own base**. A plain
/// `f64::from_str` rejects those outright, which silently skipped the
/// whole `multipleOf`/`minimum`/`maximum` step for every hex/octal/binary
/// integer (the nudge stepped by a bare ±1 and ignored the schema).
pub(crate) fn parse_repr(repr: &str, fmt: Format) -> Option<f64> {
    let clean = repr.trim().replace('_', "");
    let radix = match fmt {
        Format::Hex => 16,
        Format::Octal => 8,
        Format::Binary => 2,
        _ => return clean.parse::<f64>().ok(),
    };
    i64::from_str_radix(clean.get(2..)?, radix)
        .ok()
        .map(|n| n as f64)
}

/// Render a schema-adjusted nudge result **in the node's own notation**:
/// `format_nudged` for decimal integers/floats, the source radix prefix for
/// a hex/octal/binary integer (hex keeps its authored digit case, exactly
/// like `nudge_scalar`), and the source's underscore grouping when it had
/// any. Without this a schema-clamped nudge rewrote `0xFF` as decimal and
/// dropped `1_000`'s grouping.
pub(crate) fn format_nudged_like(n: f64, step: Option<f64>, old_repr: &str, fmt: Format) -> String {
    let out = match fmt {
        Format::Hex | Format::Octal | Format::Binary => {
            let i = n.round() as i64;
            match fmt {
                Format::Hex => {
                    let clean = old_repr.trim().replace('_', "");
                    let upper = clean
                        .get(2..)
                        .is_some_and(|d| d.chars().any(|c| c.is_ascii_uppercase()));
                    if upper {
                        format!("0x{i:X}")
                    } else {
                        format!("0x{i:x}")
                    }
                }
                Format::Octal => format!("0o{i:o}"),
                _ => format!("0b{i:b}"),
            }
        }
        _ => format_nudged(n, step, !old_repr.contains('.')),
    };
    if !old_repr.contains('_') {
        return out;
    }
    if out.contains('.') {
        regroup_float(&out)
    } else {
        regroup_int(&out, fmt)
    }
}

fn group_right(digits: &str, n: usize) -> String {
    let len = digits.chars().count();
    let mut out = String::with_capacity(len + len / n);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (len - i).is_multiple_of(n) {
            out.push('_');
        }
        out.push(c);
    }
    out
}

fn group_left(digits: &str, n: usize) -> String {
    let mut out = String::with_capacity(digits.len() + digits.len() / n);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && i.is_multiple_of(n) {
            out.push('_');
        }
        out.push(c);
    }
    out
}

fn regroup_int(repr: &str, fmt: Format) -> String {
    match fmt {
        Format::Hex | Format::Octal | Format::Binary => {
            let (prefix, digits) = repr.split_at(2);
            format!("{prefix}{}", group_right(digits, 4))
        }
        _ => {
            let (sign, digits) = repr.strip_prefix('-').map_or(("", repr), |d| ("-", d));
            format!("{sign}{}", group_right(digits, 3))
        }
    }
}

fn regroup_float(repr: &str) -> String {
    let (sign, body) = repr.strip_prefix('-').map_or(("", repr), |d| ("-", d));
    match body.split_once('.') {
        Some((int, frac)) => {
            format!("{sign}{}.{}", group_right(int, 3), group_left(frac, 3))
        }
        None => format!("{sign}{}", group_right(body, 3)),
    }
}
