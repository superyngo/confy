//! The Add-type picker (`Mode::AddPicker`) — opened by `AddNode`/`AddChild`/
//! `AddSibling` (TUI `a`, web `a`/FAB, the Action menu's "Add child"/"Append
//! sibling") instead of the old "copy the cursor's kind" heuristic. Lists the
//! legal node kinds for the resolved insertion `Target`, filtered by the
//! target parent's kind/format, and seeds the picked kind's default literal
//! once committed. Mirrors `action_menu.rs`'s shape (core-owned item list,
//! `cursor`-based, one `mode_view()` conversion, three host renderings).

use crate::model::document::{ConfigDocument, DocFormat, Mutation, Target};
use crate::model::node::{Format, NodeKind, ScalarType, Seg};
use crate::session::i18n::tr;
use crate::session::notice::Notice;
use crate::session::state::{AddKind, AddPickerState, Mode};

use super::session::Session;
use super::status_fmt::unique_key;

impl Session {
    /// `a` add: child-vs-sibling chosen from the cursor's expand state (TUI parity).
    pub fn add_node(&mut self) {
        self.add_node_impl(None);
    }

    /// Force a child insertion (Web `+` / "Add child"): always append into the
    /// cursor branch regardless of its expand state.
    pub fn add_child(&mut self) {
        self.add_node_impl(Some(true));
    }

    /// Force a sibling insertion (Web "Append sibling"): always insert after the
    /// cursor regardless of its expand state.
    pub fn add_sibling(&mut self) {
        self.add_node_impl(Some(false));
    }

    fn add_node_impl(&mut self, force_append: Option<bool>) {
        if self.guard_clipboard_locked() {
            return;
        }
        if self.doc.is_none() {
            return;
        }
        let cursor_row = match self.cursor_row() {
            Some(r) => r,
            None => return,
        };
        let expanded = self.expanded.contains(&cursor_row.path);
        let is_append = match force_append {
            Some(b) => b,
            None => cursor_row.path.is_empty() || (cursor_row.is_branch && expanded),
        };
        let target = if is_append {
            let n = self
                .tree
                .node_at(&cursor_row.path)
                .map(|p| p.children.len())
                .unwrap_or(0);
            Target {
                parent: cursor_row.path.clone(),
                index: n,
            }
        } else {
            let mut parent = cursor_row.path.clone();
            parent.pop();
            Target {
                parent,
                index: self.true_sibling_index(&cursor_row.path) + 1,
            }
        };
        // Sibling add defaults to the cursor node's own kind (old "copy
        // previous node's type" muscle memory); child add always defaults to
        // string (matches the old hard-coded child seed).
        let default_hint = if is_append {
            None
        } else {
            self.tree.node_at(&cursor_row.path).map(|n| n.kind.clone())
        };
        self.open_add_picker(target, default_hint);
    }

    fn open_add_picker(&mut self, target: Target, default_hint: Option<NodeKind>) {
        let options = self.add_picker_options(&target);
        if options.is_empty() {
            self.set_notice(Notice::core(self.lang, "core.add.unsupported", &[]));
            return;
        }
        let cursor = default_hint
            .and_then(|nk| {
                options
                    .iter()
                    .position(|(_, k)| add_kind_matches_node(k, &nk))
            })
            .or_else(|| {
                options
                    .iter()
                    .position(|(_, k)| matches!(k, AddKind::Scalar(ScalarType::String)))
            })
            .unwrap_or(0);
        self.mode = Mode::AddPicker(AddPickerState {
            target,
            options,
            cursor,
        });
    }

    /// Build the Add-type picker's option list for `target`, filtered by the
    /// resolved parent's kind/format so every listed option is legal to
    /// insert. Empty when the parent is missing or read-only (a YAML opaque
    /// node).
    fn add_picker_options(&self, target: &Target) -> Vec<(String, AddKind)> {
        let Some(doc) = self.doc.as_ref() else {
            return Vec::new();
        };
        let doc_format = doc.format();
        let Some(parent) = self.tree.node_at(&target.parent) else {
            return Vec::new();
        };
        if parent.read_only {
            return Vec::new();
        }
        let lang = self.lang;
        let mut out: Vec<(String, AddKind)> = Vec::new();
        let push = |out: &mut Vec<(String, AddKind)>, key: &'static str, k: AddKind| {
            out.push((tr(lang, key).to_string(), k));
        };

        // An `[A/T]` group's only legal child is a new `[[…]]` entry (seeded
        // as a scalar field — `insert_seed` special-cases `AddKind::Table`
        // here) or a standalone comment.
        if matches!(parent.kind, NodeKind::ArrayOfTables) {
            push(&mut out, "core.add.type.table-entry", AddKind::Table);
            push(&mut out, "core.add.type.comment", AddKind::Comment);
            return out;
        }

        let bare = matches!(parent.kind, NodeKind::Array); // keyless element context
                                                           // A flow/inline construct (TOML inline table, YAML flow map/seq) has
                                                           // no `[header]` notation and holds no comments (CONTEXT.md "Comment").
        let is_flow = matches!(parent.kind, NodeKind::InlineTable)
            || (matches!(parent.kind, NodeKind::Array) && parent.format == Format::Inline);

        push(
            &mut out,
            "core.add.type.string",
            AddKind::Scalar(ScalarType::String),
        );
        push(
            &mut out,
            "core.add.type.integer",
            AddKind::Scalar(ScalarType::Integer),
        );
        push(
            &mut out,
            "core.add.type.float",
            AddKind::Scalar(ScalarType::Float),
        );
        push(
            &mut out,
            "core.add.type.bool",
            AddKind::Scalar(ScalarType::Bool),
        );
        match doc_format {
            DocFormat::Toml => {
                push(
                    &mut out,
                    "core.add.type.offset-datetime",
                    AddKind::Scalar(ScalarType::OffsetDatetime),
                );
                push(
                    &mut out,
                    "core.add.type.local-datetime",
                    AddKind::Scalar(ScalarType::LocalDatetime),
                );
                push(
                    &mut out,
                    "core.add.type.local-date",
                    AddKind::Scalar(ScalarType::LocalDate),
                );
                push(
                    &mut out,
                    "core.add.type.local-time",
                    AddKind::Scalar(ScalarType::LocalTime),
                );
            }
            DocFormat::Json | DocFormat::Yaml => {
                push(
                    &mut out,
                    "core.add.type.null",
                    AddKind::Scalar(ScalarType::Null),
                );
            }
        }

        if doc_format == DocFormat::Toml && !bare && !is_flow {
            push(&mut out, "core.add.type.table", AddKind::Table);
            push(
                &mut out,
                "core.add.type.array-of-tables",
                AddKind::ArrayOfTables,
            );
        }
        push(
            &mut out,
            if doc_format == DocFormat::Toml {
                "core.add.type.inline-table"
            } else {
                "core.add.type.object"
            },
            AddKind::InlineTable,
        );
        push(
            &mut out,
            if doc_format == DocFormat::Yaml {
                "core.add.type.sequence"
            } else {
                "core.add.type.array"
            },
            AddKind::Array,
        );

        if !is_flow {
            push(&mut out, "core.add.type.comment", AddKind::Comment);
        }
        out
    }

    /// Moves the picker cursor by `delta`, wrapping (arrow keys/`j`/`k`).
    pub fn add_picker_move(&mut self, delta: i32) {
        if let Mode::AddPicker(st) = &mut self.mode {
            let len = st.options.len() as i32;
            if len == 0 {
                return;
            }
            st.cursor = (st.cursor as i32 + delta).rem_euclid(len) as usize;
        }
    }

    /// Jumps the picker cursor by `delta`, clamped to the option range
    /// (Home/End/PageUp/PageDown) — same convention as `schema_enum_jump`.
    pub fn add_picker_jump(&mut self, delta: i32) {
        if let Mode::AddPicker(st) = &mut self.mode {
            let len = st.options.len() as i32;
            if len == 0 {
                return;
            }
            st.cursor = (st.cursor as i32 + delta).clamp(0, len - 1) as usize;
        }
    }

    /// Commits the option under the cursor (keyboard Enter).
    pub fn add_picker_commit(&mut self) {
        let Mode::AddPicker(st) = std::mem::replace(&mut self.mode, Mode::Normal) else {
            return;
        };
        self.mode = self.resting_mode();
        let Some((_, kind)) = st.options.get(st.cursor).cloned() else {
            return;
        };
        self.insert_seed(st.target, kind);
    }

    /// Web/touch pointer analogue of `add_picker_commit`: commit a directly
    /// tapped/clicked option without moving the cursor first.
    pub fn add_picker_pick(&mut self, index: usize) {
        let Mode::AddPicker(st) = std::mem::replace(&mut self.mode, Mode::Normal) else {
            return;
        };
        self.mode = self.resting_mode();
        let Some((_, kind)) = st.options.get(index).cloned() else {
            return;
        };
        self.insert_seed(st.target, kind);
    }

    /// Closes the picker without inserting anything (`Esc`) — nothing was
    /// written yet, so unlike the old insert-then-cancel flow this never
    /// touches `History`.
    pub fn exit_add_picker(&mut self) {
        self.mode = self.resting_mode();
        self.notice = None;
    }

    /// Insert a fresh node of `kind` at `target` and move into the follow-up
    /// surface: a scalar opens the inline value editor (`created_on_add`
    /// semantics unchanged); a container is inserted inert (no rename is
    /// forced) with an auto-numbered `placeholder` key, ready to rename
    /// manually whenever.
    fn insert_seed(&mut self, mut target: Target, kind: AddKind) {
        if kind == AddKind::Comment {
            self.add_comment_sibling(target);
            return;
        }
        let parent_node = self.tree.node_at(&target.parent);
        let parent_is_array = parent_node
            .map(|n| matches!(n.kind, NodeKind::Array))
            .unwrap_or(false);
        let parent_is_aot = parent_node
            .map(|n| matches!(n.kind, NodeKind::ArrayOfTables))
            .unwrap_or(false);
        let existing: Vec<String> = parent_node
            .map(|p| p.children.iter().map(|c| c.key.clone()).collect())
            .unwrap_or_default();

        let mut seed_kind = match kind {
            AddKind::Scalar(st) => NodeKind::Scalar(st),
            AddKind::Table => NodeKind::Table,
            AddKind::ArrayOfTables => NodeKind::ArrayOfTables,
            AddKind::InlineTable => NodeKind::InlineTable,
            AddKind::Array => NodeKind::Array,
            AddKind::Comment => unreachable!("handled above"),
        };
        // The AoT group's one option ("table entry") is a new `[[…]]` entry
        // seeded with a scalar field, not a `[key]` header (invalid there) —
        // `aot_group_insert` (model/cst_edit/aot_group.rs) packs a keyed
        // scalar fragment into a fresh entry automatically.
        if parent_is_aot && kind == AddKind::Table {
            seed_kind = NodeKind::Scalar(ScalarType::String);
        }

        // A scalar into a non-array, non-AoT parent needs its slot clamped
        // ahead of the parent's first `[table]`/`[[aot]]` sub-section (TOML
        // section-ordering constraint) — same rule the old `add_node_impl` applied.
        if !parent_is_array && !parent_is_aot && matches!(seed_kind, NodeKind::Scalar(_)) {
            let split = parent_node
                .map(|p| {
                    p.children
                        .iter()
                        .position(|c| {
                            matches!(c.kind, NodeKind::Table | NodeKind::ArrayOfTables)
                                && c.format != Format::Dotted
                        })
                        .unwrap_or(p.children.len())
                })
                .unwrap_or(0);
            if target.index > split {
                target.index = split;
            }
        }

        if !target.parent.is_empty() {
            self.expanded.insert(target.parent.clone());
        }
        let doc = self.doc.as_ref().unwrap();
        let bare = parent_is_array;
        let key = if bare {
            None
        } else {
            Some(unique_key(
                if matches!(seed_kind, NodeKind::Scalar(_)) {
                    "new_field"
                } else {
                    "placeholder"
                },
                &existing,
            ))
        };
        let seed_value = |v: &str| -> String {
            if bare {
                doc.array_element_fragment(v)
            } else {
                doc.scalar_fragment(key.as_deref(), v)
            }
        };
        let (fragment, inline) = match &seed_kind {
            NodeKind::Scalar(st) => (seed_value(&scalar_seed_literal(*st)), true),
            NodeKind::Array | NodeKind::InlineTable | NodeKind::ArrayOfTables | NodeKind::Table => {
                (
                    doc.empty_container_fragment(&seed_kind, key.as_deref()),
                    false,
                )
            }
            NodeKind::Root | NodeKind::Comment(_) => unreachable!("not a selectable AddKind"),
        };
        if !self.apply_insert(target.clone(), fragment) {
            return;
        }
        let mut new_path = target.parent.clone();
        match &key {
            Some(k) => new_path.push(Seg::Key(k.clone())),
            None => new_path.push(Seg::Index(target.index)),
        }
        if self.view_row_at(&new_path).is_some() {
            self.cursor = new_path;
            if inline {
                self.begin_inline_edit();
                match &mut self.mode {
                    Mode::Edit(e) => e.created_on_add = true,
                    Mode::SchemaEnum(st) => st.created_on_add = true,
                    _ => {}
                }
            } else {
                // A freshly-added container (table/array/inline-table/AoT) is
                // never forced into rename Edit mode here, keyed or not — it
                // just gets its auto-numbered `placeholder` key (or sits bare
                // in an array) and a notice pointing at the manual rename key.
                self.set_notice(Notice::core(self.lang, "core.add.placeholder", &[]));
            }
        }
    }

    /// Insert a fresh standalone comment as a sibling at `target` and open it
    /// in the inline editor — moved verbatim from the old `add_node_impl`'s
    /// comment branch (unchanged body/behavior).
    fn add_comment_sibling(&mut self, target: Target) {
        let doc = match self.doc.as_mut() {
            Some(d) => d,
            None => return,
        };
        let text = format!("\n{} ", doc.comment_prefix());
        match doc.apply(Mutation::InsertComment {
            target: target.clone(),
            text,
        }) {
            Ok(text) => self.on_mutation_success(None, text),
            Err(e) => {
                self.set_notice(Notice::core(self.lang, "core.add.error", &[&e.to_string()]));
                return;
            }
        }
        let mut new_path = target.parent.clone();
        new_path.push(Seg::Index(target.index));
        if self.view_row_at(&new_path).is_some() {
            self.cursor = new_path;
            self.begin_inline_edit();
            match &mut self.mode {
                Mode::Edit(e) => e.created_on_add = true,
                Mode::SchemaEnum(st) => st.created_on_add = true,
                _ => {}
            }
        }
    }
}

/// The literal repr seeded into a freshly-added scalar of `st`, pre-filled
/// into the inline editor buffer before it opens (mirrors the container
/// seeds' `"placeholder"` key convention) — TOML has no `null` literal, so
/// `ScalarType::Null` is never offered for `DocFormat::Toml`
/// (`add_picker_options` excludes it). Datetime scalars seed the system
/// clock's current UTC instant (`now_*_literal`) rather than a fixed
/// 1970-01-01 stub.
fn scalar_seed_literal(st: ScalarType) -> String {
    match st {
        ScalarType::String => "\"\"".to_string(),
        ScalarType::Integer => "0".to_string(),
        ScalarType::Float => "0.0".to_string(),
        ScalarType::Bool => "false".to_string(),
        ScalarType::Null => "null".to_string(),
        ScalarType::OffsetDatetime => format!("{}Z", now_datetime_literal()),
        ScalarType::LocalDatetime => now_datetime_literal(),
        ScalarType::LocalDate => now_date_literal(),
        ScalarType::LocalTime => now_time_literal(),
    }
}

/// Current UTC "YYYY-MM-DD" — no timezone-database dependency is pulled in
/// just for this, so TOML's "no offset" datetime scalars get the clock's UTC
/// instant rather than the host machine's configured local timezone.
fn now_date_literal() -> String {
    let (y, mo, d, ..) = now_utc_parts();
    format!("{y:04}-{mo:02}-{d:02}")
}

/// Current UTC "HH:MM:SS".
fn now_time_literal() -> String {
    let (_, _, _, h, mi, s) = now_utc_parts();
    format!("{h:02}:{mi:02}:{s:02}")
}

/// Current UTC "YYYY-MM-DDTHH:MM:SS" (no offset suffix — callers append `Z`
/// themselves for `OffsetDatetime`).
fn now_datetime_literal() -> String {
    let (y, mo, d, h, mi, s) = now_utc_parts();
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}")
}

/// Current UTC calendar/clock components: `(year, month, day, hour, minute, second)`.
fn now_utc_parts() -> (i64, u32, u32, u32, u32, u32) {
    let secs = now_unix_seconds();
    let days = secs.div_euclid(86_400);
    let sod = secs.rem_euclid(86_400) as u32;
    let (y, mo, d) = civil_from_days(days);
    (y, mo, d, sod / 3600, (sod / 60) % 60, sod % 60)
}

/// Seconds since the Unix epoch, UTC. `std::time::SystemTime::now()` traps
/// at runtime on `wasm32-unknown-unknown` (no host clock import — confirmed:
/// it compiles but the call itself is an `unreachable` trap), which is the
/// target the web/touch/VS Code/Tauri UIs all run this crate as, so that
/// target reads the JS `Date.now()` clock via `js-sys` instead.
#[cfg(not(target_arch = "wasm32"))]
fn now_unix_seconds() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(target_arch = "wasm32")]
fn now_unix_seconds() -> i64 {
    (js_sys::Date::now() / 1000.0) as i64
}

/// Days-since-1970-01-01 -> `(year, month, day)`, proleptic Gregorian
/// calendar, UTC. Howard Hinnant's public-domain `civil_from_days`
/// algorithm (http://howardhinnant.github.io/date_algorithms.html) — avoids
/// pulling in a full date/timezone crate for one "seed the field with
/// today's date" feature.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// Whether `AddKind` `k` names the same notation-independent kind as
/// node-kind `nk` — used only to preselect the picker's default cursor from
/// an adjacent sibling's existing kind, never for legality (`add_picker_options`
/// already filtered to only-legal kinds).
fn add_kind_matches_node(k: &AddKind, nk: &NodeKind) -> bool {
    match (k, nk) {
        (AddKind::Scalar(a), NodeKind::Scalar(b)) => a == b,
        (AddKind::Table, NodeKind::Table) => true,
        (AddKind::ArrayOfTables, NodeKind::ArrayOfTables) => true,
        (AddKind::InlineTable, NodeKind::InlineTable) => true,
        (AddKind::Array, NodeKind::Array) => true,
        (AddKind::Comment, NodeKind::Comment(_)) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::civil_from_days;

    #[test]
    fn civil_from_days_matches_known_dates() {
        // (days since 1970-01-01, expected (y, m, d))
        let cases: &[(i64, (i64, u32, u32))] = &[
            (0, (1970, 1, 1)),
            (1, (1970, 1, 2)),
            (31, (1970, 2, 1)),
            (365, (1971, 1, 1)), // 1970 is not a leap year
            (366, (1971, 1, 2)),
            (-1, (1969, 12, 31)),
            (-365, (1969, 1, 1)),
            (20_000, (2024, 10, 4)),
        ];
        for &(days, expected) in cases {
            assert_eq!(civil_from_days(days), expected, "days={days}");
        }
    }
}
