//! Inline-editor buffer lifecycle (`begin_inline_edit*`/`edit_*`/`edit_commit`)
//! and the value/rename/nudge/add-node mutation-application methods that
//! commit through it — split out of `session.rs` (Task 15, 2026-08-11 audit
//! remediation).

use crate::model::document::{ConfigDocument, MutateError, Mutation, OnCollision, Target};
use crate::model::node::{Format, NodeKind, Path, ScalarType, Seg};
use crate::session::i18n::{tr, tr_args};
use crate::session::state::{EditField, EditState, Mode, PendingCommit, PromptKind};

use super::session::Session;

use super::schema_hint::nudge_scalar;

use super::status_fmt::{
    char_byte_idx, clamp_scroll, node_type_label_str, project_first_label, scalar_repr_for,
    unique_key,
};

impl Session {
    pub fn begin_inline_edit(&mut self) {
        self.begin_inline_edit_impl(true);
    }

    fn begin_inline_edit_impl(&mut self, allow_schema_enum: bool) {
        if self.guard_clipboard_locked() {
            return;
        }
        let row = match self.cursor_row() {
            Some(r) => r,
            None => return,
        };
        let is_comment = self
            .tree
            .node_at(&row.path)
            .map(|n| matches!(n.kind, NodeKind::Comment(_)))
            .unwrap_or(false);
        let (key, is_element) = if is_comment {
            (String::new(), false)
        } else {
            match row.path.last() {
                Some(Seg::Key(k)) => (k.clone(), false),
                Some(Seg::Index(_)) => (String::new(), true),
                None => return,
            }
        };
        let orig_trailing = if is_comment {
            None
        } else {
            row.trailing_comment.clone()
        };
        let mut buffer = row.value.clone().unwrap_or_default().trim().to_string();
        if let Some(tc) = &orig_trailing {
            buffer.push_str("  ");
            buffer.push_str(tc);
        }
        let cursor = buffer.chars().count();
        let name_cursor = key.chars().count();
        if allow_schema_enum {
            if let Some(crate::schema::EditHint::Enum(options)) = self
                .schema
                .as_ref()
                .and_then(|s| s.raw.as_ref())
                .map(|raw| crate::schema::hints_edit::resolve_edit_hint(raw, &row.path))
            {
                if !options.is_empty() {
                    let format = self.doc.as_ref().map(|d| d.format());
                    let opts: Vec<(String, String)> = options
                        .into_iter()
                        .filter_map(|(label, v)| scalar_repr_for(&v, format?).map(|r| (label, r)))
                        .collect();
                    if !opts.is_empty() {
                        self.mode = Mode::SchemaEnum(crate::session::state::SchemaEnumState {
                            path: row.path.clone(),
                            key: key.clone(),
                            is_element,
                            created_on_add: false,
                            cursor: 0,
                            options: opts,
                        });
                        self.status = None;
                        return;
                    }
                }
            }
        }
        self.mode = Mode::Edit(EditState {
            path: row.path.clone(),
            key: key.clone(),
            field: EditField::Value,
            is_element,
            is_comment,
            rename_only: false,
            buffer,
            cursor,
            scroll: 0,
            other_buffer: key,
            other_cursor: name_cursor,
            other_scroll: 0,
            orig_trailing,
            created_on_add: false,
        });
        self.status = None;
    }

    pub fn begin_inline_rename(&mut self) {
        if self.guard_clipboard_locked() {
            return;
        }
        let row = match self.cursor_row() {
            Some(r) => r,
            None => return,
        };
        let key = match row.path.last() {
            Some(Seg::Key(k)) => k.clone(),
            _ => return,
        };
        let is_comment = self
            .tree
            .node_at(&row.path)
            .map(|n| matches!(n.kind, NodeKind::Comment(_)))
            .unwrap_or(false);
        if is_comment {
            return;
        }
        let name_cursor = key.chars().count();
        self.mode = Mode::Edit(EditState {
            path: row.path.clone(),
            key: key.clone(),
            field: EditField::Name,
            is_element: false,
            is_comment: false,
            rename_only: true,
            buffer: key.clone(),
            cursor: name_cursor,
            scroll: 0,
            other_buffer: String::new(),
            other_cursor: 0,
            other_scroll: 0,
            orig_trailing: None,
            created_on_add: false,
        });
        self.status = None;
        self.error = None;
    }

    pub fn edit_toggle_field(&mut self) {
        if let Mode::Edit(ref mut e) = self.mode {
            if e.is_element || e.is_comment || e.rename_only {
                return;
            }
            std::mem::swap(&mut e.buffer, &mut e.other_buffer);
            std::mem::swap(&mut e.cursor, &mut e.other_cursor);
            std::mem::swap(&mut e.scroll, &mut e.other_scroll);
            e.field = match e.field {
                EditField::Value => EditField::Name,
                EditField::Name => EditField::Value,
            };
            self.status = None;
        }
    }

    pub fn edit_clamp_scroll(&mut self, width: usize) {
        if let Mode::Edit(ref mut e) = self.mode {
            let len = e.buffer.chars().count();
            e.scroll = clamp_scroll(e.scroll, e.cursor.min(len), len, width);
        }
    }

    pub fn edit_input_char(&mut self, c: char) {
        if let Mode::Edit(ref mut e) = self.mode {
            let byte = char_byte_idx(&e.buffer, e.cursor);
            e.buffer.insert(byte, c);
            e.cursor += 1;
            self.status = None;
        }
    }

    pub fn edit_backspace(&mut self) {
        if let Mode::Edit(ref mut e) = self.mode {
            if e.cursor > 0 {
                let prev = char_byte_idx(&e.buffer, e.cursor - 1);
                e.buffer.remove(prev);
                e.cursor -= 1;
                self.status = None;
            }
        }
    }

    pub fn edit_delete(&mut self) {
        if let Mode::Edit(ref mut e) = self.mode {
            let len = e.buffer.chars().count();
            if e.cursor < len {
                let at = char_byte_idx(&e.buffer, e.cursor);
                e.buffer.remove(at);
                self.status = None;
            }
        }
    }

    pub fn edit_cursor_left(&mut self) {
        if let Mode::Edit(ref mut e) = self.mode {
            e.cursor = e.cursor.saturating_sub(1);
        }
    }

    pub fn edit_cursor_right(&mut self) {
        if let Mode::Edit(ref mut e) = self.mode {
            let len = e.buffer.chars().count();
            if e.cursor < len {
                e.cursor += 1;
            }
        }
    }

    pub fn edit_cursor_home(&mut self) {
        if let Mode::Edit(ref mut e) = self.mode {
            e.cursor = 0;
        }
    }

    pub fn edit_cursor_end(&mut self) {
        if let Mode::Edit(ref mut e) = self.mode {
            e.cursor = e.buffer.chars().count();
        }
    }

    pub fn edit_cancel(&mut self) {
        let created_on_add = matches!(&self.mode, Mode::Edit(e) if e.created_on_add);
        self.mode = self.resting_mode();
        self.pending_edit = None;
        self.pending_trailing = None;
        self.prompt_from_commit_edit = None;
        self.status = None;
        if created_on_add {
            self.cancel_added_node();
        }
    }

    pub fn schema_enum_cancel(&mut self) {
        let created_on_add = matches!(&self.mode, Mode::SchemaEnum(st) if st.created_on_add);
        self.mode = self.resting_mode();
        self.status = None;
        if created_on_add {
            self.cancel_added_node();
        }
    }

    fn cancel_added_node(&mut self) {
        let snapshot = match self.history.as_mut().and_then(|h| h.cancel_last()) {
            Some(s) => s,
            None => return,
        };
        if let Some(doc) = self.doc.as_mut() {
            if doc.replace_from_str(&snapshot).is_ok() {
                self.tree = doc.project();
                self.revalidate_schema();
            }
        }
    }

    /// One-shot inline edit commit for the Web UI (`Intent::CommitEdit`): the
    /// pointer analogue of `begin_inline_edit` → type → `edit_commit`. Seeds a
    /// fresh `Mode::Edit` from the cursor row, overwrites the value/name buffers
    /// with the host-supplied text (`None` = keep current), then runs the full
    /// `edit_commit` — so type-change / collision / trailing-comment prompts all
    /// still fire. Inline path only (the host routes multiline/opaque through the
    /// external-edit handshake).
    pub fn commit_edit(&mut self, value: Option<String>, name: Option<String>) {
        if self.guard_clipboard_locked() {
            return;
        }
        let from_detail = matches!(self.mode, Mode::Detail);
        self.begin_inline_edit_impl(false);
        let Mode::Edit(ref mut e) = self.mode else {
            return;
        };
        if let Some(v) = value {
            e.cursor = v.chars().count();
            e.buffer = v;
        }
        if let Some(n) = name {
            e.other_cursor = n.chars().count();
            e.other_buffer = n;
        }
        // A branch node has no scalar value to replace (the panel doesn't even
        // render a Value field for one) — without this, renaming a branch's key
        // falls through to the value-replace step with an empty buffer and
        // fails to parse as a scalar.
        if self
            .tree
            .node_at(&e.path)
            .map(|n| n.is_branch())
            .unwrap_or(false)
        {
            e.rename_only = true;
        }
        self.edit_commit();
        // One-shot epilogue: the pointer host has no live editor to leave open.
        match &self.mode {
            // A retry branch (invalid value, rename failure, …) kept the edit —
            // cancel it and surface the retry message as the error instead.
            Mode::Edit(_) => {
                let msg = self.status.take();
                self.edit_cancel();
                self.error = msg;
                if from_detail {
                    self.open_detail();
                }
            }
            // Deferred to a confirmation prompt — mark it one-shot so the
            // resolution doesn't fall back into `Mode::Edit` either.
            Mode::Prompt(_) => {
                self.prompt_from_commit_edit = Some(from_detail);
            }
            // Committed (or cleanly rejected) — a Detail-origin edit returns to
            // the panel instead of dropping to Normal, so the panel stays open.
            _ => {
                if from_detail {
                    self.open_detail();
                }
            }
        }
    }

    pub fn edit_commit(&mut self) {
        let rest = self.resting_mode();
        let mut e = match std::mem::replace(&mut self.mode, rest) {
            Mode::Edit(e) => e,
            other => {
                self.mode = other;
                return;
            }
        };
        // Comment node: commit via EditComment.
        if e.is_comment {
            let text = e.buffer.clone();
            let ok = match self.doc.as_mut() {
                Some(doc) => doc.apply(Mutation::EditComment {
                    path: e.path.clone(),
                    text,
                }),
                None => Ok(()),
            };
            match ok {
                Ok(()) => self.on_mutation_success(None),
                Err(MutateError::Fragment(msg)) => {
                    self.status = Some(tr_args(self.lang, "core.comment.invalid", &[&msg]));
                    self.mode = Mode::Edit(e);
                }
                Err(err) => {
                    self.status = Some(tr_args(
                        self.lang,
                        "core.error.generic",
                        &[&err.to_string()],
                    ));
                    self.mode = Mode::Edit(e);
                }
            }
            return;
        }
        let (name_str, raw_value) = match e.field {
            EditField::Value => (e.other_buffer.clone(), e.buffer.clone()),
            EditField::Name => (e.buffer.clone(), e.other_buffer.clone()),
        };
        let is_element = matches!(e.path.last(), Some(Seg::Index(_)));
        let split = self
            .doc
            .as_ref()
            .filter(|d| d.supports_comments())
            .map(|d| d.split_value_comment(&raw_value));
        let (value_str, new_trailing) = match split {
            Some((v, c)) => (v, c),
            None => (raw_value.clone(), None),
        };
        if new_trailing.is_some() {
            let in_inline = (1..e.path.len()).any(|i| {
                self.tree
                    .node_at(&e.path[..i])
                    .map(|n| {
                        matches!(n.kind, NodeKind::InlineTable)
                            || (matches!(n.kind, NodeKind::Array) && n.format == Format::Inline)
                    })
                    .unwrap_or(false)
            });
            if in_inline {
                self.status = Some(tr(self.lang, "core.trailing.inline-unsupported").to_string());
                self.mode = Mode::Edit(e);
                return;
            }
        }
        let preserves = self
            .doc
            .as_ref()
            .map(|d| d.replace_preserves_trailing_comment())
            .unwrap_or(true);
        let changed = new_trailing != e.orig_trailing;
        let reassert = !preserves && new_trailing.is_some();
        self.pending_trailing = (changed || reassert).then_some(new_trailing);
        let mut frag_key = if is_element {
            "__elem__".to_string()
        } else {
            e.key.clone()
        };
        // 1. Key rename (Name field changed).
        if !is_element {
            let new_name = name_str.trim().to_string();
            if new_name != e.key {
                if new_name.is_empty() {
                    self.status = Some(tr(self.lang, "core.rename.empty-key").to_string());
                    self.mode = Mode::Edit(e);
                    return;
                }
                let old_label = node_type_label_str(
                    &self
                        .tree
                        .node_at(&e.path)
                        .map(|n| n.kind.clone())
                        .unwrap_or(NodeKind::Root),
                )
                .to_string();
                let new_label = self
                    .doc
                    .as_ref()
                    .map(|d| d.rename_can_change_type())
                    .unwrap_or(false)
                    .then(|| project_first_label(&format!("{new_name} = {value_str}\n")))
                    .flatten();
                if let Some(new_label) = new_label {
                    if new_label != old_label {
                        self.status = Some(tr_args(
                            self.lang,
                            "core.type-change",
                            &[&old_label, &new_label],
                        ));
                        self.pending_edit = Some((
                            e,
                            PendingCommit::Rename {
                                new_name,
                                value: value_str,
                            },
                        ));
                        self.mode = Mode::Prompt(PromptKind::TypeChange {
                            from: old_label,
                            to: new_label,
                        });
                        return;
                    }
                }
                let res = match self.doc.as_mut() {
                    Some(doc) => doc.apply(Mutation::Rename {
                        path: e.path.clone(),
                        new_key: new_name.clone(),
                    }),
                    None => Ok(()),
                };
                match res {
                    Ok(()) => {
                        self.on_mutation_success(None);
                        let old_path = e.path.clone();
                        if let Some(last) = e.path.last_mut() {
                            *last = Seg::Key(new_name.clone());
                        }
                        // Keep the cursor -- and any selected/anchored paths
                        // under it -- on the renamed node (its identity is its
                        // path) instead of letting them go stale or snap away.
                        if self.cursor == old_path {
                            self.cursor = e.path.clone();
                        }
                        self.selection.remap_prefix(&old_path, &e.path);
                        e.key = new_name.clone();
                        frag_key = new_name;
                    }
                    Err(err) => {
                        self.status = Some(tr_args(
                            self.lang,
                            "core.rename.failed",
                            &[&err.to_string()],
                        ));
                        self.mode = Mode::Edit(e);
                        return;
                    }
                }
            }
        }
        // F2 rename-only: skip value Replace.
        if e.rename_only {
            self.mode = self.resting_mode();
            return;
        }
        // 2. Value replace.
        let key_arg = (!is_element).then_some(frag_key.as_str());
        let (fragment, new_label) = match self.doc.as_ref() {
            Some(doc) => {
                let fragment = doc.scalar_fragment(key_arg, &value_str);
                match doc.value_kind(&value_str) {
                    Ok(kind) => (fragment, node_type_label_str(&kind).to_string()),
                    Err(msg) => {
                        self.status = Some(tr_args(self.lang, "core.value.invalid", &[&msg]));
                        self.mode = Mode::Edit(e);
                        return;
                    }
                }
            }
            None => (format!("{frag_key} = {value_str}\n"), String::new()),
        };
        let old_label = node_type_label_str(
            &self
                .tree
                .node_at(&e.path)
                .map(|n| n.kind.clone())
                .unwrap_or(NodeKind::Root),
        )
        .to_string();
        if new_label != old_label {
            self.status = Some(tr_args(
                self.lang,
                "core.type-change",
                &[&old_label, &new_label],
            ));
            self.pending_edit = Some((e, PendingCommit::Replace(fragment)));
            self.mode = Mode::Prompt(PromptKind::TypeChange {
                from: old_label,
                to: new_label,
            });
            return;
        }
        self.apply_replace(e.path, fragment);
    }

    pub(crate) fn apply_deferred_rename(
        &mut self,
        mut e: EditState,
        new_name: String,
        value: String,
    ) {
        let res = match self.doc.as_mut() {
            Some(doc) => doc.apply(Mutation::Rename {
                path: e.path.clone(),
                new_key: new_name.clone(),
            }),
            None => return,
        };
        if let Err(err) = res {
            self.error = Some(tr_args(
                self.lang,
                "core.rename.failed",
                &[&err.to_string()],
            ));
            return;
        }
        self.on_mutation_success(None);
        let old_path = e.path.clone();
        let parent_len = e.path.len() - 1;
        let new_segs: Vec<Seg> = new_name
            .split('.')
            .map(|s| Seg::Key(s.to_string()))
            .collect();
        let leaf_key = match new_segs.last() {
            Some(Seg::Key(k)) => k.clone(),
            _ => new_name.clone(),
        };
        e.path.truncate(parent_len);
        e.path.extend(new_segs);
        // Keep the cursor -- and any selected/anchored paths under it -- on
        // the renamed node (path identity changed).
        if self.cursor == old_path {
            self.cursor = e.path.clone();
        }
        self.selection.remap_prefix(&old_path, &e.path);
        self.apply_replace(e.path, format!("{leaf_key} = {value}\n"));
    }

    pub fn apply_replace(&mut self, path: Path, edited: String) {
        let trailing = self.pending_trailing.take();
        let doc = match self.doc.as_mut() {
            Some(d) => d,
            None => return,
        };
        let fmt = doc.format().name();
        match doc.apply(Mutation::Replace {
            path: path.clone(),
            fragment: edited,
        }) {
            Ok(()) => {
                if let Some(comment) = trailing {
                    if let Err(e) = doc.apply(Mutation::SetTrailingComment {
                        path: path.clone(),
                        comment,
                    }) {
                        self.error = Some(tr_args(
                            self.lang,
                            "core.trailing.update-failed",
                            &[&e.to_string()],
                        ));
                    }
                }
                self.on_mutation_success(Some(&path));
                self.note_schema_violation(&path);
            }
            Err(MutateError::Fragment(msg)) => {
                self.error = Some(tr_args(self.lang, "core.fragment.invalid", &[fmt, &msg]));
            }
            Err(e) => {
                self.error = Some(tr_args(self.lang, "core.error.generic", &[&e.to_string()]))
            }
        }
    }

    /// Set/change/clear a node's trailing inline comment (Web `SetTrailing`:
    /// the separate comment cell + "Append comment"). Atomic + semantically
    /// validated by `Mutation::SetTrailingComment`; an unsupported target
    /// (inline collection, …) leaves the document untouched and reports the
    /// error as a status message.
    pub fn set_trailing_comment(&mut self, path: Path, comment: Option<String>) {
        let doc = match self.doc.as_mut() {
            Some(d) => d,
            None => return,
        };
        // The Web panel sends the raw typed text; `SetTrailingComment` expects the
        // comment WITH its marker ("# foo" / "// foo"). Normalize: drop empties to a
        // clear (None), and prepend the backend's marker when it's missing.
        let prefix = doc.comment_prefix();
        let comment = comment.and_then(|c| {
            let t = c.trim();
            if t.is_empty() {
                None
            } else if t.starts_with(prefix) {
                Some(t.to_string())
            } else {
                Some(format!("{prefix} {t}"))
            }
        });
        match doc.apply(Mutation::SetTrailingComment { path, comment }) {
            Ok(()) => self.on_mutation_success(None),
            Err(e) => {
                self.error = Some(tr_args(
                    self.lang,
                    "core.trailing.update-failed",
                    &[&e.to_string()],
                ))
            }
        }
    }

    pub fn apply_edit_comment(&mut self, path: Path, text: String) {
        let doc = match self.doc.as_mut() {
            Some(d) => d,
            None => return,
        };
        match doc.apply(Mutation::EditComment { path, text }) {
            Ok(()) => self.on_mutation_success(None),
            Err(MutateError::Fragment(msg)) => {
                self.error = Some(tr_args(self.lang, "core.comment.invalid", &[&msg]));
            }
            Err(e) => {
                self.error = Some(tr_args(self.lang, "core.error.generic", &[&e.to_string()]))
            }
        }
    }

    /// Clamp/snap an arrow-key-nudged value into a schema's `Bounded`
    /// constraint. Returns the (possibly adjusted) repr to commit. The
    /// early-return cases — no schema, no raw schema text, no `Bounded`
    /// hint for this path, or `new_repr` not parsing as a number — all
    /// pass the value through unchanged (mirroring today's early-`true`
    /// guards), so `None` is never produced in practice today; the
    /// `Option` wrapper is kept for the `let Some(..) else { return }`
    /// call-site shape. The arrow-key nudge *clamps* to `[minimum, maximum]`
    /// and *snaps* to `multiple_of` (spec §3 `Bounded{min,max,multiple_of}`
    /// row); free-text inline typing stays unclamped (this is
    /// arrow-key-nudge-only — an out-of-range typed value is flagged by
    /// validate.rs, never rejected at commit).
    pub(crate) fn schema_clamp_nudge(
        &self,
        path: &crate::model::node::Path,
        new_repr: &str,
    ) -> Option<String> {
        let Some(state) = self.schema.as_ref() else {
            return Some(new_repr.to_string());
        };
        let Some(raw) = state.raw.as_ref() else {
            return Some(new_repr.to_string());
        };
        let hint = crate::schema::hints_edit::resolve_edit_hint(raw, path);
        let crate::schema::EditHint::Bounded {
            minimum,
            maximum,
            multiple_of,
        } = hint
        else {
            return Some(new_repr.to_string());
        };
        let Ok(mut n) = new_repr.replace('_', "").parse::<f64>() else {
            return Some(new_repr.to_string());
        };
        // Snap to the nearest multiple of `step` (positive steps only — a
        // non-positive multipleOf is invalid JSON Schema, ignored).
        if let Some(step) = multiple_of {
            if step > 0.0 {
                n = (n / step).round() * step;
            }
        }
        // Clamp to [minimum, maximum] (whichever bounds are present).
        if let Some(min) = minimum {
            n = n.max(min);
        }
        if let Some(max) = maximum {
            n = n.min(max);
        }
        // Format back, matching the original repr's style when sensible:
        // an integer-style repr (no '.') yielding a whole number formats
        // as an integer; otherwise a plain float format. Best-effort
        // display value — the committed representation comes from
        // re-parsing the fragment downstream.
        let was_int_style = !new_repr.contains('.');
        Some(if was_int_style && n.fract() == 0.0 {
            format!("{}", n as i64)
        } else {
            format!("{n}")
        })
    }

    pub fn nudge(&mut self, delta: i64) {
        if self.guard_clipboard_locked() {
            return;
        }
        let path = match self.cursor_row() {
            Some(r) => r.path,
            None => return,
        };
        let frag_key = match path.last() {
            Some(Seg::Key(k)) => k.clone(),
            Some(Seg::Index(_)) => {
                let fi = path
                    .iter()
                    .position(|s| matches!(s, Seg::Index(_)))
                    .unwrap_or(0);
                if path[fi..].iter().all(|s| matches!(s, Seg::Index(_))) {
                    "__elem__".to_string()
                } else {
                    return;
                }
            }
            _ => return,
        };
        let node = match self.tree.node_at(&path) {
            Some(n) => n,
            None => return,
        };
        let st = match node.kind {
            NodeKind::Scalar(st) => st,
            _ => return,
        };
        let repr = match &node.value {
            Some(v) => v.clone(),
            None => return,
        };
        let format = node.format;
        let trailing = node.trailing_comment.clone();
        if let Some(new_repr) = nudge_scalar(st, format, &repr, delta) {
            let Some(new_repr) = self.schema_clamp_nudge(&path, &new_repr) else {
                return;
            };
            let key_arg = (frag_key != "__elem__").then_some(frag_key.as_str());
            let preserves = self
                .doc
                .as_ref()
                .map(|d| d.replace_preserves_trailing_comment())
                .unwrap_or(true);
            let fragment = match self.doc.as_ref() {
                Some(doc) => doc.scalar_fragment(key_arg, &new_repr),
                None => format!("{frag_key} = {new_repr}\n"),
            };
            if !preserves {
                if let Some(tc) = trailing {
                    self.pending_trailing = Some(Some(tc));
                }
            }
            self.apply_replace(path, fragment);
        }
    }

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
        let cursor_kind = self.tree.node_at(&cursor_row.path).map(|n| n.kind.clone());
        let mut target = if is_append {
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
        let parent_node = self.tree.node_at(&target.parent);
        let parent_is_array = parent_node
            .map(|n| matches!(n.kind, NodeKind::Array))
            .unwrap_or(false);
        let existing: Vec<String> = parent_node
            .map(|p| p.children.iter().map(|c| c.key.clone()).collect())
            .unwrap_or_default();
        let seed_kind = if is_append {
            NodeKind::Scalar(ScalarType::String)
        } else {
            cursor_kind.unwrap_or(NodeKind::Scalar(ScalarType::String))
        };
        if matches!(seed_kind, NodeKind::Comment(_)) {
            self.add_comment_sibling(target);
            return;
        }
        if is_append && !parent_is_array && matches!(seed_kind, NodeKind::Scalar(_)) {
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
            NodeKind::Scalar(_) | NodeKind::Root | NodeKind::Comment(_) => {
                (seed_value("\"\""), true)
            }
            NodeKind::Array | NodeKind::InlineTable | NodeKind::ArrayOfTables | NodeKind::Table => {
                (
                    doc.empty_container_fragment(&seed_kind, key.as_deref()),
                    false,
                )
            }
        };
        self.apply_insert(target.clone(), fragment);
        if self.error.is_some() {
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
            } else if key.is_some() {
                // Container sibling with a key: enter rename mode so the user
                // can immediately rename the placeholder key and, crucially,
                // pressing Escape triggers `edit_cancel → cancel_added_node`,
                // removing the just-inserted container (same UX as AddChild).
                self.begin_inline_rename();
                if let Mode::Edit(e) = &mut self.mode {
                    e.created_on_add = true;
                }
            } else {
                self.status = Some(tr(self.lang, "core.add.placeholder").to_string());
            }
        }
    }

    fn add_comment_sibling(&mut self, target: Target) {
        let doc = match self.doc.as_mut() {
            Some(d) => d,
            None => return,
        };
        if !doc.supports_comments() {
            self.status = Some(tr(self.lang, "core.comment.unsupported").to_string());
            return;
        }
        // A leading blank line keeps the new comment a *separate* single-line node
        // instead of merging into the adjacent comment (consecutive `#` lines
        // project as one node; a blank splits them).
        let text = format!("\n{} ", doc.comment_prefix());
        match doc.apply(Mutation::InsertComment {
            target: target.clone(),
            text,
        }) {
            Ok(()) => self.on_mutation_success(None),
            Err(e) => {
                self.error = Some(tr_args(self.lang, "core.add.error", &[&e.to_string()]));
                return;
            }
        }
        let mut new_path = target.parent.clone();
        new_path.push(Seg::Index(target.index));
        if self.view_row_at(&new_path).is_some() {
            self.cursor = new_path;
            // Enter the inline editor on the fresh comment so the user types
            // immediately; `created_on_add` makes Esc remove it (and its
            // blank-line separator) via History::cancel_last, matching scalar add.
            self.begin_inline_edit();
            match &mut self.mode {
                Mode::Edit(e) => e.created_on_add = true,
                Mode::SchemaEnum(st) => st.created_on_add = true,
                _ => {}
            }
        }
    }

    pub fn apply_insert(&mut self, target: Target, edited: String) {
        let doc = match self.doc.as_mut() {
            Some(d) => d,
            None => return,
        };
        let fmt = doc.format().name();
        match doc.apply(Mutation::Insert {
            target,
            fragment: edited,
            on_collision: OnCollision::Cancel,
        }) {
            Ok(()) => self.on_mutation_success(None),
            Err(MutateError::Collision(key)) => {
                self.error = Some(tr_args(self.lang, "core.insert.collision", &[&key]));
            }
            Err(MutateError::Fragment(msg)) => {
                self.error = Some(tr_args(self.lang, "core.fragment.invalid", &[fmt, &msg]));
            }
            Err(e) => {
                self.error = Some(tr_args(self.lang, "core.error.generic", &[&e.to_string()]))
            }
        }
    }

    /// Commits the picked enum/const value. Deliberately routes through
    /// `edit_commit` (like the Web one-shot `commit_edit`) rather than
    /// applying the `Replace` mutation directly, so a schema-picked value
    /// that would change the node's underlying type (e.g. an enum mixing
    /// string and numeric consts) gets the same `Mode::Prompt(TypeChange)`
    /// confirmation gate as any other value commit — previously this path
    /// bypassed it entirely (every surface, not just one host, since this is
    /// core-shared logic). `allow_schema_enum: false` keeps `edit_commit`
    /// from re-diverting back into the picker.
    pub fn schema_enum_commit(&mut self) {
        let Mode::SchemaEnum(st) = std::mem::replace(&mut self.mode, Mode::Normal) else {
            return;
        };
        let Some((_, value_repr)) = st.options.get(st.cursor).cloned() else {
            return;
        };
        self.cursor = st.path.clone();
        self.begin_inline_edit_impl(false);
        let Mode::Edit(e) = &mut self.mode else {
            return;
        };
        // Seed the buffer with the picked value plus whatever trailing
        // comment already lived on the line (mirrors the buffer `begin_
        // inline_edit_impl` would have built had it entered the picker
        // itself, spec §1525) so `edit_commit`'s unchanged-trailing check
        // sees no diff and never disturbs it.
        let mut buffer = value_repr;
        if let Some(tc) = &e.orig_trailing {
            buffer.push_str("  ");
            buffer.push_str(tc);
        }
        e.cursor = buffer.chars().count();
        e.buffer = buffer;
        self.edit_commit();
        // One-shot epilogue, mirroring `commit_edit`: the picker has no live
        // text editor or Detail panel to fall back into on decline/retry —
        // schema_enum_commit already always resolved to `Mode::Normal` on
        // success, so both outcomes settle there too.
        match &self.mode {
            Mode::Edit(_) => {
                let msg = self.status.take();
                self.edit_cancel();
                self.error = msg;
            }
            Mode::Prompt(_) => self.prompt_from_commit_edit = Some(false),
            _ => {}
        }
    }
}
