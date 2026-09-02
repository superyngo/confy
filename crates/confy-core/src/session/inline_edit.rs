//! Inline-editor buffer lifecycle (`begin_inline_edit*`/`edit_*`/`edit_commit`)
//! and the value/rename/nudge/add-node mutation-application methods that
//! commit through it — split out of `session.rs` (Task 15, 2026-08-11 audit
//! remediation).

use crate::model::document::{ConfigDocument, MutateError, Mutation, OnCollision, Target};
use crate::model::node::{Format, NodeKind, Path, ScalarType, Seg};
use crate::session::notice::Notice;
use crate::session::state::{EditField, EditState, Mode, PendingCommit, PromptKind};

use super::session::Session;

use super::schema_hint::{format_nudged, nudge_scalar};

use super::status_fmt::{
    char_byte_idx, clamp_scroll, node_type_label_str, project_first_label, scalar_repr_for,
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
        // The key edits as its **authored spelling** (`ViewRow.key_literal`) —
        // see `begin_inline_rename` for the full rationale. `None` (comment /
        // element rows) falls back to the decoded `key`.
        let edit_key_text = row.key_literal.clone().unwrap_or_else(|| key.clone());
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
        let name_cursor = edit_key_text.chars().count();
        if allow_schema_enum {
            let format = self.doc.as_ref().map(|d| d.format());
            // 1. A real schema `enum`/`const` constraint always wins — the
            //    schema is the authority on the value domain, including for a
            //    `bool`-typed node (its `enum` may legitimately be one-sided,
            //    or a differently-spelled/typed set), so the boolean fallback
            //    below only ever fires when the schema says nothing.
            if let Some(crate::schema::EditHint::Enum(options)) = self
                .schema
                .as_ref()
                .and_then(|s| s.raw.as_ref())
                .map(|raw| crate::schema::hints_edit::resolve_edit_hint(raw, &row.path))
            {
                if !options.is_empty() {
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
                            from_schema: true,
                        });
                        self.notice = None;
                        return;
                    }
                }
            }
            // 2. Schema-independent fallback: a `bool` scalar's value domain is
            //    closed at two members, so it picks rather than types — the same
            //    picker widget/mode as a schema `enum` on every host (TUI popup,
            //    web `<select>`, touch sheet), differing only in the popup title
            //    (`from_schema: false`). Free-form text entry for a `bool` stays
            //    reachable through the external/popup editor
            //    (`BeginEditExternal`, TUI `E`, the panel's "Editor" button),
            //    which never routes through here.
            if row.scalar_type == Some(ScalarType::Bool) {
                if let Some((opts, cursor)) =
                    bool_picker_options(row.value.as_deref().unwrap_or_default())
                {
                    self.mode = Mode::SchemaEnum(crate::session::state::SchemaEnumState {
                        path: row.path.clone(),
                        key: key.clone(),
                        is_element,
                        created_on_add: false,
                        cursor,
                        options: opts,
                        from_schema: false,
                    });
                    self.notice = None;
                    return;
                }
            }
        }
        self.mode = Mode::Edit(EditState {
            path: row.path.clone(),
            key: edit_key_text.clone(),
            field: EditField::Value,
            is_element,
            is_comment,
            rename_only: false,
            buffer,
            cursor,
            scroll: 0,
            other_buffer: edit_key_text,
            other_cursor: name_cursor,
            other_scroll: 0,
            orig_trailing,
            created_on_add: false,
        });
        self.notice = None;
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
        // The key edits as its **authored spelling** (`ViewRow.key_literal`) —
        // quotes and escapes intact — not the decoded `key`. That makes the
        // quote characters themselves directly editable and keeps an
        // inside-quote trailing space from being eaten by the caller's `.trim()`
        // on commit. `None` (keyless rows) falls back to the decoded key.
        let edit_text = row.key_literal.clone().unwrap_or(key);
        let name_cursor = edit_text.chars().count();
        self.mode = Mode::Edit(EditState {
            path: row.path.clone(),
            key: edit_text.clone(),
            field: EditField::Name,
            is_element: false,
            is_comment: false,
            rename_only: true,
            buffer: edit_text,
            cursor: name_cursor,
            scroll: 0,
            other_buffer: String::new(),
            other_cursor: 0,
            other_scroll: 0,
            orig_trailing: None,
            created_on_add: false,
        });
        self.notice = None;
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
            self.notice = None;
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
            self.notice = None;
        }
    }

    pub fn edit_backspace(&mut self) {
        if let Mode::Edit(ref mut e) = self.mode {
            if e.cursor > 0 {
                let prev = char_byte_idx(&e.buffer, e.cursor - 1);
                e.buffer.remove(prev);
                e.cursor -= 1;
                self.notice = None;
            }
        }
    }

    pub fn edit_delete(&mut self) {
        if let Mode::Edit(ref mut e) = self.mode {
            let len = e.buffer.chars().count();
            if e.cursor < len {
                let at = char_byte_idx(&e.buffer, e.cursor);
                e.buffer.remove(at);
                self.notice = None;
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
        self.notice = None;
        if created_on_add {
            self.cancel_added_node();
        }
    }

    pub fn schema_enum_cancel(&mut self) {
        let created_on_add = matches!(&self.mode, Mode::SchemaEnum(st) if st.created_on_add);
        self.mode = self.resting_mode();
        self.notice = None;
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
                let msg = self.notice.take();
                self.edit_cancel();
                if let Some(n) = msg {
                    self.set_notice(n);
                }
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
                None => Ok(String::new()),
            };
            match ok {
                Ok(text) => self.on_mutation_success(None, text),
                Err(MutateError::Fragment(msg)) => {
                    self.set_notice(Notice::core(self.lang, "core.comment.invalid", &[&msg]));
                    self.mode = Mode::Edit(e);
                }
                Err(err) => {
                    self.set_notice(Notice::core(
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
        let split = self.doc.as_ref().map(|d| d.split_value_comment(&raw_value));
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
                self.set_notice(Notice::core(
                    self.lang,
                    "core.trailing.inline-unsupported",
                    &[],
                ));
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
                    self.set_notice(Notice::core(self.lang, "core.rename.empty-key", &[]));
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
                    None => Ok(String::new()),
                };
                match res {
                    Ok(text) => {
                        self.on_mutation_success(None, text);
                        // The document now holds `new_name` verbatim, but a
                        // projected path is built from DECODED keys -- so the
                        // cursor/selection must be re-anchored on the decoded
                        // form, or every later `node_at` misses (a spurious
                        // type-change prompt, then "path not found").
                        self.remap_renamed_path(&mut e.path, &new_name);
                        e.key = new_name.clone();
                        frag_key = new_name;
                    }
                    Err(err) => {
                        self.set_notice(Notice::core(
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
                        self.set_notice(Notice::core(self.lang, "core.value.invalid", &[&msg]));
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
            self.pending_edit = Some((e, PendingCommit::Replace(fragment)));
            self.mode = Mode::Prompt(PromptKind::TypeChange {
                from: old_label,
                to: new_label,
            });
            return;
        }
        self.apply_replace(e.path, fragment);
    }

    /// Re-anchor `path` — and the cursor/selection riding on it — onto a node
    /// just renamed to the **literal** `new_key`.
    ///
    /// The document stores `new_key` verbatim (quotes, escapes and all), while a
    /// projected path is made of DECODED segments, so the backend's own key
    /// lexer supplies the new segments. Never `split('.')` here: a quoted key is
    /// allowed to contain a dot.
    fn remap_renamed_path(&mut self, path: &mut Vec<Seg>, new_key: &str) {
        if path.is_empty() {
            return;
        }
        let segs: Vec<Seg> = self
            .doc
            .as_ref()
            .map(|d| d.rename_key_segs(new_key))
            .unwrap_or_default()
            .into_iter()
            .map(Seg::Key)
            .collect();
        // An unparseable key can't reach here (`Mutation::Rename` rejected it
        // first), but never corrupt the path if it somehow does.
        if segs.is_empty() {
            return;
        }
        let old_path = path.clone();
        path.truncate(old_path.len() - 1);
        path.extend(segs);
        // The renamed node's identity IS its path, so anything anchored on the
        // old one follows it instead of going stale or snapping away.
        if self.cursor == old_path {
            self.cursor = path.clone();
        }
        self.selection.remap_prefix(&old_path, path);
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
        let text = match res {
            Ok(t) => t,
            Err(err) => {
                self.set_notice(Notice::core(
                    self.lang,
                    "core.rename.failed",
                    &[&err.to_string()],
                ));
                return;
            }
        };
        self.on_mutation_success(None, text);
        // The re-written fragment needs the LEAF's literal spelling. A
        // single-segment rename is its own leaf (exact even when the key is
        // quoted around a dot); only a dotted TOML rename has to be split.
        let seg_count = self
            .doc
            .as_ref()
            .map(|d| d.rename_key_segs(&new_name).len())
            .unwrap_or(1);
        let leaf_key = if seg_count <= 1 {
            new_name.clone()
        } else {
            new_name.rsplit('.').next().unwrap_or(&new_name).to_string()
        };
        self.remap_renamed_path(&mut e.path, &new_name);
        self.apply_replace(e.path, format!("{leaf_key} = {value}\n"));
    }

    /// External-editor commit (host popup / TUI `$EDITOR`): `text` is the
    /// fragment's complete, authoritative representation, unlike the inline
    /// editor's value-only fragment (which manages the comment separately via
    /// `pending_trailing`). If the node had a trailing comment before this
    /// edit and the returned fragment doesn't write one, the user explicitly
    /// deleted it in their editor — force the clear rather than falling
    /// through to `Replace`'s "preserve the old comment when the fragment is
    /// silent about it" default (comment-advisory follow-up issue #4).
    pub fn apply_external_replace(&mut self, path: Path, text: String) {
        let had_comment = self
            .tree
            .node_at(&path)
            .and_then(|n| n.trailing_comment.clone());
        if had_comment.is_some() {
            let new_comment = self
                .doc
                .as_ref()
                .and_then(|d| d.fragment_trailing_comment(&path, &text));
            if new_comment.is_none() {
                self.pending_trailing = Some(None);
            }
        }
        self.apply_replace(path, text);
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
            Ok(mut text) => {
                if let Some(comment) = trailing {
                    match doc.apply(Mutation::SetTrailingComment {
                        path: path.clone(),
                        comment,
                    }) {
                        Ok(t2) => text = t2,
                        Err(e) => {
                            self.set_notice(Notice::core(
                                self.lang,
                                "core.trailing.update-failed",
                                &[&e.to_string()],
                            ));
                        }
                    }
                }
                self.on_mutation_success(Some(&path), text);
                self.note_schema_violation(&path);
            }
            Err(MutateError::Fragment(msg)) => {
                self.set_notice(Notice::core(
                    self.lang,
                    "core.fragment.invalid",
                    &[fmt, &msg],
                ));
            }
            Err(e) => self.set_notice(Notice::core(
                self.lang,
                "core.error.generic",
                &[&e.to_string()],
            )),
        }
    }

    /// Set/change/clear a node's trailing inline comment (Web `SetTrailing`:
    /// the separate comment cell + "Append comment"). Atomic + semantically
    /// validated by `Mutation::SetTrailingComment`; an unsupported target
    /// (inline collection, …) leaves the document untouched and reports the
    /// error as a notice.
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
            Ok(text) => self.on_mutation_success(None, text),
            Err(e) => {
                self.set_notice(Notice::core(
                    self.lang,
                    "core.trailing.update-failed",
                    &[&e.to_string()],
                ));
            }
        }
    }

    pub fn apply_edit_comment(&mut self, path: Path, text: String) {
        let doc = match self.doc.as_mut() {
            Some(d) => d,
            None => return,
        };
        match doc.apply(Mutation::EditComment { path, text }) {
            Ok(text) => self.on_mutation_success(None, text),
            Err(MutateError::Fragment(msg)) => {
                self.set_notice(Notice::core(self.lang, "core.comment.invalid", &[&msg]));
            }
            Err(e) => self.set_notice(Notice::core(
                self.lang,
                "core.error.generic",
                &[&e.to_string()],
            )),
        }
    }

    /// Step/clamp a nudged value onto a schema's `Bounded` constraint.
    /// `old_repr` is the pre-nudge repr, `new_repr` the `nudge_scalar`
    /// result, `delta` the requested number of steps; returns the
    /// (possibly adjusted) repr to commit. The early-return cases — no
    /// schema, no raw schema text, no `Bounded` hint for this path, or
    /// `new_repr` not parsing as a number — all pass the value through
    /// unchanged (mirroring today's early-`true` guards), so `None` is
    /// never produced in practice today; the `Option` wrapper is kept for
    /// the `let Some(..) else { return }` call-site shape.
    ///
    /// With a `multipleOf` the nudge **steps along that grid** instead of
    /// stepping by one and snapping to the nearest multiple: on a grid
    /// coarser than 2 the snap lands right back on the value the step came
    /// from, which froze the nudge entirely (`poll_ms = 253` with
    /// `multipleOf: 5` was stuck at 255 in *both* directions). An off-grid
    /// value aligns *in the nudge's direction* on the first step (253 → 255
    /// going up, 250 going down) and then moves whole steps. Bounds clamp
    /// inward to the nearest in-range grid point, so parking at a bound
    /// never leaves a value the schema itself rejects. Without a
    /// `multipleOf` this is a plain `[minimum, maximum]` clamp, as before
    /// (spec §3 `Bounded{min,max,multiple_of}` row). Free-text inline
    /// typing stays unclamped (this is nudge-only — an out-of-range typed
    /// value is flagged by validate.rs, never rejected at commit).
    pub(crate) fn schema_clamp_nudge(
        &self,
        path: &crate::model::node::Path,
        old_repr: &str,
        new_repr: &str,
        delta: i64,
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
        // An integer-style repr must stay an integer: a fractional
        // `multipleOf` would otherwise nudge an Integer node into a Float.
        // Positive steps only — a non-positive multipleOf is invalid JSON
        // Schema, ignored.
        let int_style = !old_repr.contains('.');
        let grid = multiple_of.filter(|s| *s > 0.0 && (!int_style || s.fract() == 0.0));
        let old = old_repr.replace('_', "").parse::<f64>().ok();
        if let (Some(step), Some(o), true) = (grid, old, delta != 0) {
            let u = o / step;
            let units = if (u - u.round()).abs() <= 1e-9 * u.abs().max(1.0) {
                // already on the grid: move `delta` whole steps
                u.round() + delta as f64
            } else if delta > 0 {
                // off-grid: the first step is the directional alignment
                u.ceil() + (delta - 1) as f64
            } else {
                u.floor() + (delta + 1) as f64
            };
            n = units * step;
            // Clamp inside the grid, so a bound can't park on a value the
            // schema rejects (and can't oscillate against the snap).
            if let Some(min) = minimum {
                if n < min {
                    n = (min / step).ceil() * step;
                }
            }
            if let Some(max) = maximum {
                if n > max {
                    n = (max / step).floor() * step;
                }
            }
            return Some(format_nudged(n, Some(step), int_style));
        }
        // No usable grid (or a directionless call): snap to the nearest
        // multiple, then clamp to [minimum, maximum].
        if let Some(step) = grid {
            n = (n / step).round() * step;
        }
        if let Some(min) = minimum {
            n = n.max(min);
        }
        if let Some(max) = maximum {
            n = n.min(max);
        }
        Some(format_nudged(n, grid, int_style))
    }

    /// Stateless preview of nudging `text` — the host's *current edit-buffer*
    /// string, which may differ from the committed node value — by `delta`
    /// steps, without mutating the document or session mode. `None` when
    /// `path` isn't a nudgeable scalar (bool/string/datetime — see
    /// `nudge_scalar`) or `text` doesn't parse for its type. Read-only sibling
    /// of `nudge()`: same `nudge_scalar` + `schema_clamp_nudge` pipeline, but
    /// the caller decides whether/when to commit the result. Used by the Web/
    /// touch wheel and swipe nudge while inline-editing (WEBUI.md), which
    /// writes the result straight into the focused `<input>` and commits once
    /// via the normal `CommitEdit` path rather than dispatching per tick.
    pub fn nudge_repr(&self, path: &Path, text: &str, delta: i64) -> Option<String> {
        let node = self.tree.node_at(path)?;
        let st = match node.kind {
            NodeKind::Scalar(st) => st,
            _ => return None,
        };
        let new_repr = nudge_scalar(st, node.format, text, delta)?;
        self.schema_clamp_nudge(path, text, &new_repr, delta)
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
            let Some(new_repr) = self.schema_clamp_nudge(&path, &repr, &new_repr, delta) else {
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

    /// Apply a hand-typed insert fragment. Returns `false` when the insert
    /// failed (collision / invalid fragment / generic error — the failure
    /// notice is already set); `true` otherwise.
    pub fn apply_insert(&mut self, target: Target, edited: String) -> bool {
        let doc = match self.doc.as_mut() {
            Some(d) => d,
            None => return true,
        };
        let fmt = doc.format().name();
        match doc.apply(Mutation::Insert {
            target,
            fragment: edited,
            on_collision: OnCollision::Cancel,
            // Hand-typed fragment with no source path to derive an
            // array-element key from — generic placeholder fallback,
            // exactly the pre-feature behavior.
            suggested_key: None,
        }) {
            Ok(text) => {
                self.on_mutation_success(None, text);
                true
            }
            Err(MutateError::Collision(key)) => {
                self.set_notice(Notice::core(self.lang, "core.insert.collision", &[&key]));
                false
            }
            Err(MutateError::Fragment(msg)) => {
                self.set_notice(Notice::core(
                    self.lang,
                    "core.fragment.invalid",
                    &[fmt, &msg],
                ));
                false
            }
            Err(e) => {
                self.set_notice(Notice::core(
                    self.lang,
                    "core.error.generic",
                    &[&e.to_string()],
                ));
                false
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
                let msg = self.notice.take();
                self.edit_cancel();
                if let Some(n) = msg {
                    self.set_notice(n);
                }
            }
            Mode::Prompt(_) => self.prompt_from_commit_edit = Some(false),
            _ => {}
        }
    }
}

/// The two options (and the index of the current one) for the schema-independent
/// `bool` value picker, in the node's **own authored spelling** — YAML accepts
/// `true`/`True`/`TRUE` (and the `false` counterparts) as booleans
/// (`model/yaml/project.rs`), so offering only the lowercase pair would silently
/// re-case an authored `TRUE` on commit. Both option labels and their value
/// reprs follow the current repr's casing; an unrecognized spelling returns
/// `None`, which falls back to plain text editing (same
/// conservative-if-unsure polarity as `nudge_scalar`).
fn bool_picker_options(repr: &str) -> Option<(Vec<(String, String)>, usize)> {
    let (t, f, cursor) = match repr.trim() {
        "true" => ("true", "false", 0),
        "false" => ("true", "false", 1),
        "True" => ("True", "False", 0),
        "False" => ("True", "False", 1),
        "TRUE" => ("TRUE", "FALSE", 0),
        "FALSE" => ("TRUE", "FALSE", 1),
        _ => return None,
    };
    Some((
        vec![
            (t.to_string(), t.to_string()),
            (f.to_string(), f.to_string()),
        ],
        cursor,
    ))
}
