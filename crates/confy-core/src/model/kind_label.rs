//! The one label format every kind/type picker option is rendered in.
//!
//! A picker row is `"<name>  <sample>"` — a short human name for the kind or
//! notation, then an example of what the value will look like (`0x…`,
//! `"""…"""`, `[A/M]`, a rendered datetime literal). The name column is padded
//! so every sample in one list starts at the same offset, which is why this is
//! a helper and not a `format!` at each call site: the padding depends on the
//! whole list, and it used to be hand-typed into the label literals (so a new
//! option silently broke the column, and the datetime picker — whose names are
//! *translated*, hence variable-width — could not do it at all).
//!
//! Width is measured in terminal display cells, not chars: a translated name
//! ("位移日期時間") is half as many chars as it is columns wide.
//!
//! Consumers: `ConfigDocument::kind_options` in all three backends (the `K`
//! notation list) and `Session::datetime_picker_state` (ADR 0012's datetime
//! type list). Padding is computed over the options **actually offered**, after
//! the current notation/type has been filtered out, so there is never a column
//! of trailing space belonging to an absent row.

use unicode_width::UnicodeWidthStr;

/// Pad `rows`' names to a common width and join each to its sample, keeping
/// every row's target payload. A row with an empty sample is just its name (no
/// trailing padding).
pub fn align_options<N: AsRef<str>, S: AsRef<str>, T>(rows: Vec<(N, S, T)>) -> Vec<(String, T)> {
    let width = rows
        .iter()
        .filter(|(_, s, _)| !s.as_ref().is_empty())
        .map(|(n, _, _)| n.as_ref().width())
        .max()
        .unwrap_or(0);
    rows.into_iter()
        .map(|(name, sample, target)| {
            let name = name.as_ref();
            let sample = sample.as_ref();
            if sample.is_empty() {
                return (name.to_string(), target);
            }
            let pad = " ".repeat(width.saturating_sub(name.width()));
            (format!("{name}{pad}  {sample}"), target)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::align_options;

    #[test]
    fn samples_start_at_one_column() {
        let out = align_options(vec![("hex", "0x…", 1), ("binary", "0b…", 2)]);
        assert_eq!(out[0].0, "hex     0x…");
        assert_eq!(out[1].0, "binary  0b…");
        assert_eq!(
            out[0].0.find('0').unwrap(),
            out[1].0.find('0').unwrap(),
            "columns must line up"
        );
        assert_eq!((out[0].1, out[1].1), (1, 2), "payloads survive");
    }

    #[test]
    fn a_sampleless_row_gets_no_padding() {
        let out = align_options(vec![("decimal", "", 1), ("hex", "0x…", 2)]);
        assert_eq!(out[0].0, "decimal");
    }

    /// A CJK name is padded by display cells, so the column still lines up in a
    /// terminal (a char-count pad would leave it short by one per CJK char).
    #[test]
    fn padding_counts_display_cells_not_chars() {
        let out = align_options(vec![("日期", "1979-05-27", 1), ("date time", "…", 2)]);
        assert_eq!(out[0].0, "日期       1979-05-27");
        assert_eq!(out[1].0, "date time  …");
    }
}
