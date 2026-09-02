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
