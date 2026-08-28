//! Undo/redo — split out of `session.rs` (Task 15, 2026-08-11 audit
//! remediation).

use crate::model::document::ConfigDocument;
use crate::session::notice::Notice;

use super::session::Session;

impl Session {
    pub fn undo(&mut self) {
        if self.guard_clipboard_locked() {
            return;
        }
        let snapshot = match self.history.as_mut().and_then(|h| h.undo()) {
            Some(s) => s,
            None => {
                self.set_notice(Notice::core(self.lang, "core.undo.empty", &[]));
                return;
            }
        };
        let doc = match self.doc.as_mut() {
            Some(d) => d,
            None => return,
        };
        match doc.replace_from_str(&snapshot) {
            Ok(()) => {
                self.tree = doc.project();
                self.notice = None;
                self.revalidate_schema();
                self.sync_schema_hint();
            }
            Err(e) => self.set_notice(Notice::core(
                self.lang,
                "core.undo.error",
                &[&e.to_string()],
            )),
        }
    }

    pub fn redo(&mut self) {
        if self.guard_clipboard_locked() {
            return;
        }
        let snapshot = match self.history.as_mut().and_then(|h| h.redo()) {
            Some(s) => s,
            None => {
                self.set_notice(Notice::core(self.lang, "core.redo.empty", &[]));
                return;
            }
        };
        let doc = match self.doc.as_mut() {
            Some(d) => d,
            None => return,
        };
        match doc.replace_from_str(&snapshot) {
            Ok(()) => {
                self.tree = doc.project();
                self.notice = None;
                self.revalidate_schema();
                self.sync_schema_hint();
            }
            Err(e) => self.set_notice(Notice::core(
                self.lang,
                "core.redo.error",
                &[&e.to_string()],
            )),
        }
    }
}
