use crate::model::node::Path;

/// Drop any selected path that is a descendant of another selected path (§6.2).
pub fn normalize(mut paths: Vec<Path>) -> Vec<Path> {
    paths.sort_by_key(|p| p.len());
    let mut kept: Vec<Path> = Vec::new();
    for p in paths {
        let is_descendant = kept.iter().any(|anc| p.len() > anc.len() && p.starts_with(anc));
        if !is_descendant { kept.push(p); }
    }
    kept
}

/// Multi-row selection state.
pub struct Selection {
    pub indices: std::collections::HashSet<usize>,
    pub anchor: Option<usize>,
}

impl Selection {
    pub fn new() -> Self {
        Selection { indices: std::collections::HashSet::new(), anchor: None }
    }

    /// Toggle selection at the given row index and set/clear anchor.
    pub fn toggle(&mut self, idx: usize) {
        if self.indices.remove(&idx) {
            if self.anchor == Some(idx) { self.anchor = None; }
        } else {
            self.indices.insert(idx);
            self.anchor = Some(idx);
        }
    }

    /// Extend selection from anchor to `to`, inclusive.
    pub fn extend_to(&mut self, to: usize) {
        let anchor = match self.anchor {
            Some(a) => a,
            None => { self.anchor = Some(to); self.indices.insert(to); return; }
        };
        let (lo, hi) = if anchor <= to { (anchor, to) } else { (to, anchor) };
        for i in lo..=hi { self.indices.insert(i); }
    }

    pub fn clear(&mut self) {
        self.indices.clear();
        self.anchor = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::node::Seg;

    #[test]
    fn normalize_drops_selected_descendants() {
        // selected: [server], [server.port]  -> port dropped (carried by server)
        let server = vec![Seg::Key("server".into())];
        let port = vec![Seg::Key("server".into()), Seg::Key("port".into())];
        let normalized = normalize(vec![server.clone(), port]);
        assert_eq!(normalized, vec![server]);
    }
}
