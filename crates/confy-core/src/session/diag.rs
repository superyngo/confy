//! Diagnostics ring — see ADR 0008 and design spec §7. Developer-facing,
//! English-only, no i18n. Zero new dependencies (no `tracing`/`log`).

use std::collections::VecDeque;

const CAPACITY: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagLevel {
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone)]
pub struct DiagEvent {
    pub seq: u64,
    pub level: DiagLevel,
    pub kind: &'static str, // "dispatch" | "mutation" | "schema" | "convert" | "notice"
    pub detail: String,
}

#[derive(Debug, Default)]
pub struct DiagRing {
    events: VecDeque<DiagEvent>,
    next_seq: u64,
}

impl DiagRing {
    pub fn push(&mut self, level: DiagLevel, kind: &'static str, detail: String) {
        if self.events.len() == CAPACITY {
            self.events.pop_front();
        }
        self.events.push_back(DiagEvent { seq: self.next_seq, level, kind, detail });
        self.next_seq += 1;
    }

    pub fn iter(&self) -> impl Iterator<Item = &DiagEvent> {
        self.events.iter()
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_evicts_oldest_past_256() {
        let mut ring = DiagRing::default();
        for i in 0..260 {
            ring.push(DiagLevel::Debug, "dispatch", format!("intent={i}"));
        }
        let events: Vec<_> = ring.iter().collect();
        assert_eq!(events.len(), 256);
        assert_eq!(events.first().unwrap().detail, "intent=4"); // oldest 4 evicted
        assert_eq!(events.last().unwrap().detail, "intent=259");
    }

    #[test]
    fn seq_is_monotonic() {
        let mut ring = DiagRing::default();
        ring.push(DiagLevel::Info, "notice", "a".into());
        ring.push(DiagLevel::Info, "notice", "b".into());
        let events: Vec<_> = ring.iter().collect();
        assert_eq!(events[1].seq, events[0].seq + 1);
    }
}
