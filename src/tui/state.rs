/// Multi-step undo/redo over full serialized-document snapshots.
/// One snapshot per user action (§6 z/y). UI-state changes never push.
pub struct History {
    past: Vec<String>,
    current: String,
    future: Vec<String>,
}

impl History {
    pub fn new(initial: String) -> Self {
        History { past: Vec::new(), current: initial, future: Vec::new() }
    }
    pub fn push(&mut self, snapshot: String) {
        self.past.push(std::mem::replace(&mut self.current, snapshot));
        self.future.clear();
    }
    pub fn undo(&mut self) -> Option<String> {
        let prev = self.past.pop()?;
        self.future.push(std::mem::replace(&mut self.current, prev.clone()));
        Some(prev)
    }
    pub fn redo(&mut self) -> Option<String> {
        let next = self.future.pop()?;
        self.past.push(std::mem::replace(&mut self.current, next.clone()));
        Some(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn undo_redo_restores_snapshots() {
        let mut h = History::new("v0".to_string());
        h.push("v1".to_string()); // snapshot AFTER an action
        h.push("v2".to_string());
        assert_eq!(h.undo(), Some("v1".to_string()));
        assert_eq!(h.undo(), Some("v0".to_string()));
        assert_eq!(h.undo(), None);
        assert_eq!(h.redo(), Some("v1".to_string()));
    }
}
