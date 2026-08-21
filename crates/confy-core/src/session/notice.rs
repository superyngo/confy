//! Notice model — the single-slot, user-facing transient message. See
//! `CONTEXT.md` § Messages & diagnostics.

use super::i18n::{tr_args, Lang};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Success,
    Warn,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NoticeSource {
    Core,
    HostTui,
    HostWeb,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Notice {
    pub severity: Severity,
    pub text: String,
    pub source: NoticeSource,
}

impl Notice {
    pub fn core(lang: Lang, key: &str, args: &[&str]) -> Self {
        Notice { severity: severity_of(key), text: tr_args(lang, key, args), source: NoticeSource::Core }
    }
    pub fn host_tui(lang: Lang, key: &str, args: &[&str]) -> Self {
        Notice { severity: severity_of(key), text: tr_args(lang, key, args), source: NoticeSource::HostTui }
    }
    pub fn host_web(lang: Lang, key: &str, args: &[&str]) -> Self {
        Notice { severity: severity_of(key), text: tr_args(lang, key, args), source: NoticeSource::HostWeb }
    }
}

/// Single source of truth for a catalog key's tier (§2.2 of the design spec).
/// Every `core.*`/host-notice key MUST appear here before it can be used in
/// a `Notice::*` constructor — there is no explicit-severity escape hatch.
pub fn severity_of(key: &str) -> Severity {
    match key {
        "core.error.generic" | "core.add.error" | "core.delete.error" | "core.paste.error"
        | "core.paste.comment-illegal" | "core.remark.error" | "core.rename.failed"
        | "core.trailing.update-failed" | "core.undo.error" | "core.redo.error"
        | "core.kind-switch.error"
        | "tui.host.convert-write-failed" | "tui.host.editor-error" | "tui.host.no-save-path"
        | "tui.host.save-error" | "tui.lang.save-failed" => Severity::Error,

        "core.readonly" | "core.clipboard.action-locked" | "core.comment.unsupported"
        | "core.trailing.inline-unsupported" | "core.reveal.hidden-by-filter" | "core.move.self"
        | "core.insert.collision" | "core.rename.empty-key" | "core.value.invalid"
        | "core.comment.invalid" | "core.fragment.invalid" | "core.remark.invalid"
        | "core.convert.root-only" | "core.kind-switch.unsupported" | "core.schema.violation"
        | "web.host.fxios-save-hint" | "tui.host.readonly-comment" => Severity::Warn,

        "core.save.saved" | "core.kind-switch.converted" | "core.kind-switch.converted-generic"
        | "core.clipboard.cut" | "core.clipboard.copied" | "core.clipboard.cut-changed"
        | "core.clipboard.copied-changed"
        | "web.host.save-ok" | "web.host.download-ok" | "web.host.delete.ok"
        | "web.host.add.node" | "web.host.add.child" | "web.host.add.sibling"
        | "web.host.kind.changed" | "web.host.value.changed"
        | "tui.host.saved" | "tui.host.convert-success" => Severity::Success,

        "core.save.nothing" | "core.clipboard.empty" | "core.clipboard.cleared"
        | "core.selection.cleared" | "core.undo.empty" | "core.redo.empty"
        | "core.paste.cancelled" | "core.add.placeholder" | "core.convert.aborted"
        | "web.host.kind.no-options" | "tui.host.no-changes" => Severity::Info,
        _ => panic!("severity_of: unmapped notice key {key:?} — add it to the table in notice.rs"),
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_of_covers_the_full_catalog_table() {
        // One assertion per §2.2 row. Keep this table byte-identical to the
        // spec's §2.2 groups — it IS the single source of truth once Task 4
        // deletes the inline severities at call sites.
        let cases: &[(&str, Severity)] = &[
            ("core.error.generic", Severity::Error),
            ("core.add.error", Severity::Error),
            ("core.delete.error", Severity::Error),
            ("core.paste.error", Severity::Error),
            ("core.paste.comment-illegal", Severity::Error),
            ("core.remark.error", Severity::Error),
            ("core.rename.failed", Severity::Error),
            ("core.trailing.update-failed", Severity::Error),
            ("core.undo.error", Severity::Error),
            ("core.redo.error", Severity::Error),
            ("core.kind-switch.error", Severity::Error),
            ("core.readonly", Severity::Warn),
            ("core.clipboard.action-locked", Severity::Warn),
            ("core.comment.unsupported", Severity::Warn),
            ("core.trailing.inline-unsupported", Severity::Warn),
            ("core.reveal.hidden-by-filter", Severity::Warn),
            ("core.move.self", Severity::Warn),
            ("core.insert.collision", Severity::Warn),
            ("core.rename.empty-key", Severity::Warn),
            ("core.value.invalid", Severity::Warn),
            ("core.comment.invalid", Severity::Warn),
            ("core.fragment.invalid", Severity::Warn),
            ("core.remark.invalid", Severity::Warn),
            ("core.convert.root-only", Severity::Warn),
            ("core.kind-switch.unsupported", Severity::Warn),
            ("core.schema.violation", Severity::Warn),
            ("core.save.saved", Severity::Success),
            ("core.kind-switch.converted", Severity::Success),
            ("core.kind-switch.converted-generic", Severity::Success),
            ("core.clipboard.cut", Severity::Success),
            ("core.clipboard.copied", Severity::Success),
            ("core.clipboard.cut-changed", Severity::Success),
            ("core.clipboard.copied-changed", Severity::Success),
            ("core.save.nothing", Severity::Info),
            ("core.clipboard.empty", Severity::Info),
            ("core.clipboard.cleared", Severity::Info),
            ("core.selection.cleared", Severity::Info),
            ("core.undo.empty", Severity::Info),
            ("core.redo.empty", Severity::Info),
            ("core.paste.cancelled", Severity::Info),
            ("core.add.placeholder", Severity::Info),
            ("core.convert.aborted", Severity::Info),
        ];
        assert_eq!(cases.len(), 42, "42 keys: §2.2's 41 (11 Error + 14 Warn + 7 Success + 9 Info) + controller-approved core.schema.violation (pass-through wrapper for the dynamic schema-violation advisory)");
        for (key, expected) in cases {
            assert_eq!(severity_of(key), *expected, "key {key} classified wrong");
        }
    }

    #[test]
    fn notice_core_derives_severity_from_key() {
        let n = Notice::core(Lang::En, "core.save.saved", &[]);
        assert_eq!(n.severity, Severity::Success);
        assert_eq!(n.source, NoticeSource::Core);
    }
}
