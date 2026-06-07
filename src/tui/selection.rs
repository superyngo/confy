use crate::model::node::Path;

/// Drop any selected path that is a descendant of another selected path (§6.2).
pub fn normalize(mut paths: Vec<Path>) -> Vec<Path> {
    paths.sort_by_key(|p| p.len());
    let mut kept: Vec<Path> = Vec::new();
    for p in paths {
        let is_descendant = kept
            .iter()
            .any(|anc| p.len() > anc.len() && p.starts_with(anc));
        if !is_descendant {
            kept.push(p);
        }
    }
    kept
}

/// Multi-row selection state.
///
/// A shift-drag builds a single contiguous `round` (anchor..=cursor). When a new
/// round starts (a non-shift key broke the previous run of shift+arrows) the old
/// round is folded into `committed`, so successive rounds *union* together —
/// separate runs stay separate, overlapping runs merge. `s` toggles a single row
/// straight into `committed`. The live selection is `committed ∪ round`.
pub struct Selection {
    committed: std::collections::HashSet<usize>,
    round: std::collections::HashSet<usize>,
    anchor: Option<usize>,
}

impl Default for Selection {
    fn default() -> Self {
        Self::new()
    }
}

impl Selection {
    pub fn new() -> Self {
        Selection {
            committed: std::collections::HashSet::new(),
            round: std::collections::HashSet::new(),
            anchor: None,
        }
    }

    /// Iterate the live selection (committed rows plus the in-progress round).
    pub fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.committed.union(&self.round).copied()
    }

    pub fn contains(&self, idx: usize) -> bool {
        self.committed.contains(&idx) || self.round.contains(&idx)
    }

    pub fn is_empty(&self) -> bool {
        self.committed.is_empty() && self.round.is_empty()
    }

    /// Fold the current round into the committed set and forget the anchor.
    fn commit_round(&mut self) {
        for i in self.round.drain() {
            self.committed.insert(i);
        }
        self.anchor = None;
    }

    /// Toggle a single row (bound to `s`): finalize any open round, then flip the
    /// row in the committed set.
    pub fn toggle(&mut self, idx: usize) {
        self.commit_round();
        if !self.committed.remove(&idx) {
            self.committed.insert(idx);
        }
    }

    /// Start a fresh shift round anchored at `anchor`, folding the previous round
    /// into the committed set first so rounds union rather than replace.
    pub fn begin_round(&mut self, anchor: usize) {
        self.commit_round();
        self.anchor = Some(anchor);
        self.round.insert(anchor);
    }

    /// Set the current round to exactly anchor..=`to`, inclusive. This replaces
    /// (not unions into) the round, so shrinking it with the opposite arrow
    /// deselects the rows left behind — while committed rounds are untouched.
    pub fn extend_round_to(&mut self, to: usize) {
        let anchor = match self.anchor {
            Some(a) => a,
            None => {
                self.anchor = Some(to);
                self.round.clear();
                self.round.insert(to);
                return;
            }
        };
        self.round.clear();
        let (lo, hi) = if anchor <= to {
            (anchor, to)
        } else {
            (to, anchor)
        };
        for i in lo..=hi {
            self.round.insert(i);
        }
    }

    pub fn clear(&mut self) {
        self.committed.clear();
        self.round.clear();
        self.anchor = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::node::Seg;

    fn selected(sel: &Selection) -> std::collections::HashSet<usize> {
        sel.iter().collect()
    }

    #[test]
    fn round_replaces_range_while_extending() {
        use std::collections::HashSet;
        let mut sel = Selection::new();
        sel.begin_round(3);
        sel.extend_round_to(6); // round 3..=6
        assert_eq!(selected(&sel), HashSet::from([3, 4, 5, 6]));
        sel.extend_round_to(4); // shrink within the same round
        assert_eq!(selected(&sel), HashSet::from([3, 4]));
    }

    #[test]
    fn separate_rounds_union_not_extend() {
        use std::collections::HashSet;
        let mut sel = Selection::new();
        // round 1: rows 1..=2
        sel.begin_round(1);
        sel.extend_round_to(2);
        // a new round starting at row 5 must NOT extend from round-1's anchor.
        sel.begin_round(5);
        sel.extend_round_to(6);
        assert_eq!(selected(&sel), HashSet::from([1, 2, 5, 6]));
    }

    #[test]
    fn overlapping_rounds_merge() {
        use std::collections::HashSet;
        let mut sel = Selection::new();
        sel.begin_round(1);
        sel.extend_round_to(3); // {1,2,3}
        sel.begin_round(3);
        sel.extend_round_to(5); // {3,4,5} unions -> {1..5}
        assert_eq!(selected(&sel), HashSet::from([1, 2, 3, 4, 5]));
    }

    #[test]
    fn toggle_finalizes_round_then_flips_row() {
        use std::collections::HashSet;
        let mut sel = Selection::new();
        sel.begin_round(1);
        sel.extend_round_to(2); // {1,2}
        sel.toggle(5); // commit round, add 5
        assert_eq!(selected(&sel), HashSet::from([1, 2, 5]));
        sel.toggle(1); // remove 1
        assert_eq!(selected(&sel), HashSet::from([2, 5]));
    }

    #[test]
    fn normalize_drops_selected_descendants() {
        // selected: [server], [server.port]  -> port dropped (carried by server)
        let server = vec![Seg::Key("server".into())];
        let port = vec![Seg::Key("server".into()), Seg::Key("port".into())];
        let normalized = normalize(vec![server.clone(), port]);
        assert_eq!(normalized, vec![server]);
    }
}
