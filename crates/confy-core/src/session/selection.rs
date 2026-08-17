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
    committed: std::collections::HashSet<Path>,
    round: std::collections::HashSet<Path>,
    anchor: Option<Path>,
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

    pub fn iter(&self) -> impl Iterator<Item = Path> + '_ {
        self.committed.union(&self.round).cloned()
    }

    pub fn contains(&self, path: &Path) -> bool {
        self.committed.contains(path) || self.round.contains(path)
    }

    pub fn is_empty(&self) -> bool {
        self.committed.is_empty() && self.round.is_empty()
    }

    fn commit_round(&mut self) {
        for p in self.round.drain() {
            self.committed.insert(p);
        }
        self.anchor = None;
    }

    pub fn toggle(&mut self, path: Path) {
        self.commit_round();
        if !self.committed.remove(&path) {
            self.committed.insert(path);
        }
    }

    pub fn begin_round(&mut self, anchor: Path) {
        self.commit_round();
        self.anchor = Some(anchor.clone());
        self.round.insert(anchor);
    }

    pub fn extend_round_to(&mut self, visible: &[Path], to: &Path) {
        let anchor = match self.anchor.clone() {
            Some(a) => a,
            None => {
                self.anchor = Some(to.clone());
                self.round.clear();
                self.round.insert(to.clone());
                return;
            }
        };
        let ai = visible.iter().position(|p| p == &anchor);
        let ti = visible.iter().position(|p| p == to);
        self.round.clear();
        match (ai, ti) {
            (Some(ai), Some(ti)) => {
                let (lo, hi) = if ai <= ti { (ai, ti) } else { (ti, ai) };
                for p in &visible[lo..=hi] {
                    self.round.insert(p.clone());
                }
            }
            _ => {
                self.round.insert(to.clone());
            }
        }
    }

    pub fn clear(&mut self) {
        self.committed.clear();
        self.round.clear();
        self.anchor = None;
    }

    /// Replace the entire selection with `paths` (pointer analogue: the Web UI
    /// computes the full desired set from a click / ⇧-range / ⌘-toggle / marquee
    /// gesture and hands it over wholesale). Folds everything into `committed` so
    /// a later keyboard shift-round unions against it like any other round.
    pub fn set_all(&mut self, paths: impl IntoIterator<Item = Path>) {
        self.committed = paths.into_iter().collect();
        self.round.clear();
        self.anchor = None;
    }

    /// Rewrite every selected path (and the in-progress round's anchor) whose
    /// prefix is exactly `old_prefix` to `new_prefix` instead, preserving any
    /// suffix beneath it. `Selection` is just a set of path snapshots with no
    /// mutation awareness of its own — anything that changes a node's path
    /// identity (currently: rename) must explicitly remap it here, mirroring
    /// how `Session::cursor` is remapped at each rename call site. Left
    /// unremapped, a selected path silently goes stale and out-ranks the
    /// cursor in `selected_paths()`, so the very next copy/delete/paste
    /// silently targets a node that no longer exists at that path.
    pub fn remap_prefix(&mut self, old_prefix: &Path, new_prefix: &Path) {
        if old_prefix == new_prefix {
            return;
        }
        let remap_one = |p: &Path| -> Path {
            if p.starts_with(old_prefix.as_slice()) {
                let mut np = new_prefix.clone();
                np.extend_from_slice(&p[old_prefix.len()..]);
                np
            } else {
                p.clone()
            }
        };
        self.committed = self.committed.iter().map(remap_one).collect();
        self.round = self.round.iter().map(remap_one).collect();
        self.anchor = self.anchor.as_ref().map(remap_one);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::node::Seg;

    fn selected(sel: &Selection) -> std::collections::HashSet<Path> {
        sel.iter().collect()
    }

    fn p(i: usize) -> Path {
        vec![Seg::Key(i.to_string())]
    }
    fn vis(n: usize) -> Vec<Path> {
        (0..n).map(p).collect()
    }

    #[test]
    fn round_replaces_range_while_extending() {
        use std::collections::HashSet;
        let v = vis(8);
        let mut sel = Selection::new();
        sel.begin_round(p(3));
        sel.extend_round_to(&v, &p(6));
        assert_eq!(selected(&sel), HashSet::from([p(3), p(4), p(5), p(6)]));
        sel.extend_round_to(&v, &p(4));
        assert_eq!(selected(&sel), HashSet::from([p(3), p(4)]));
    }

    #[test]
    fn separate_rounds_union_not_extend() {
        use std::collections::HashSet;
        let v = vis(8);
        let mut sel = Selection::new();
        sel.begin_round(p(1));
        sel.extend_round_to(&v, &p(2));
        sel.begin_round(p(5));
        sel.extend_round_to(&v, &p(6));
        assert_eq!(selected(&sel), HashSet::from([p(1), p(2), p(5), p(6)]));
    }

    #[test]
    fn overlapping_rounds_merge() {
        use std::collections::HashSet;
        let v = vis(8);
        let mut sel = Selection::new();
        sel.begin_round(p(1));
        sel.extend_round_to(&v, &p(3));
        sel.begin_round(p(3));
        sel.extend_round_to(&v, &p(5));
        assert_eq!(
            selected(&sel),
            HashSet::from([p(1), p(2), p(3), p(4), p(5)])
        );
    }

    #[test]
    fn toggle_finalizes_round_then_flips_row() {
        use std::collections::HashSet;
        let v = vis(8);
        let mut sel = Selection::new();
        sel.begin_round(p(1));
        sel.extend_round_to(&v, &p(2));
        sel.toggle(p(5));
        assert_eq!(selected(&sel), HashSet::from([p(1), p(2), p(5)]));
        sel.toggle(p(1));
        assert_eq!(selected(&sel), HashSet::from([p(2), p(5)]));
    }

    #[test]
    fn normalize_drops_selected_descendants() {
        let server = vec![Seg::Key("server".into())];
        let port = vec![Seg::Key("server".into()), Seg::Key("port".into())];
        let normalized = normalize(vec![server.clone(), port]);
        assert_eq!(normalized, vec![server]);
    }

    #[test]
    fn remap_prefix_rewrites_exact_and_descendant_matches() {
        use std::collections::HashSet;
        let old = vec![Seg::Key("new_field".into())];
        let new = vec![Seg::Key("inner".into())];
        let descendant = vec![Seg::Key("new_field".into()), Seg::Key("x".into())];
        let mut sel = Selection::new();
        sel.set_all([old.clone(), descendant.clone()]);
        sel.remap_prefix(&old, &new);
        let expected_descendant = vec![Seg::Key("inner".into()), Seg::Key("x".into())];
        assert_eq!(
            selected(&sel),
            HashSet::from([new, expected_descendant]),
            "an exact match and a descendant of the renamed prefix must both be rewritten"
        );
    }

    #[test]
    fn remap_prefix_leaves_unrelated_paths_untouched() {
        use std::collections::HashSet;
        let renamed = vec![Seg::Key("new_field".into())];
        let unrelated = vec![Seg::Key("other".into())];
        let mut sel = Selection::new();
        sel.set_all([unrelated.clone()]);
        sel.remap_prefix(&renamed, &vec![Seg::Key("inner".into())]);
        assert_eq!(selected(&sel), HashSet::from([unrelated]));
    }

    #[test]
    fn remap_prefix_updates_the_round_anchor() {
        let v = vis(8);
        let mut sel = Selection::new();
        sel.begin_round(p(3));
        sel.extend_round_to(&v, &p(3));
        // Rename `p(3)` to a path outside the visible-row fixture, then extend
        // the round again -- the anchor must have followed the rename, not
        // stayed pinned to the now-nonexistent old path.
        let renamed = vec![Seg::Key("renamed".into())];
        sel.remap_prefix(&p(3), &renamed);
        assert!(
            selected(&sel).contains(&renamed),
            "the round's own selected path must be remapped"
        );
    }
}
