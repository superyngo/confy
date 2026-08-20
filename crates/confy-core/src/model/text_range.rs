//! Shared span helpers for projections.

/// Convert a rowan `TextRange` (UTF-8 byte offsets, half-open) to a `Range<usize>`.
pub(crate) fn to_range(r: rowan::TextRange) -> std::ops::Range<usize> {
    let start: usize = r.start().into();
    let end: usize = r.end().into();
    start..end
}
