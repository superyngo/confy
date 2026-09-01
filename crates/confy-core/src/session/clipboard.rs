//! Cut/copy/paste + the paste collision/array-upgrade prompt sub-state-
//! machine — split out of `session.rs` (Task 15, 2026-08-11 audit
//! remediation).

use crate::model::document::{ConfigDocument, MutateError, Mutation, OnCollision, Target};
use crate::model::node::{NodeKind, Path};
use crate::session::notice::Notice;
use crate::session::state::{Clipboard, Mode, PasteSlot, PromptKind};

use super::session::Session;

impl Session {
    pub fn delete_selected(&mut self) {
        if self.guard_clipboard_locked() {
            return;
        }
        if self.cursor_is_read_only() {
            self.set_notice(Notice::core(self.lang, "core.readonly", &[]));
            return;
        }
        let paths = self.selected_paths();
        if paths.is_empty() {
            return;
        }
        let mut paths = paths;
        paths.sort_by_key(|b| std::cmp::Reverse(b.len()));
        // Row index of the topmost deletion target — the cursor snaps back
        // here below when its own path was among the deleted nodes. Without
        // this, `compute_rows` sees an unresolvable cursor and falls back to
        // `rows.first()`, throwing the cursor to the top of the document.
        let rows_before = self.visible_rows();
        let first_idx = paths
            .iter()
            .filter_map(|p| rows_before.iter().position(|r| &r.path == p))
            .min();
        // A cursor *inside* a deleted subtree dies with it, so test by prefix
        // rather than equality.
        let cursor_deleted = paths.iter().any(|p| self.cursor.starts_with(p));
        let doc = match self.doc.as_mut() {
            Some(d) => d,
            None => return,
        };
        let mut text = String::new();
        for p in &paths {
            match doc.apply(Mutation::Delete { path: p.clone() }) {
                Ok(t) => text = t,
                Err(e) => {
                    self.set_notice(Notice::core(
                        self.lang,
                        "core.delete.error",
                        &[&e.to_string()],
                    ));
                    return;
                }
            }
        }
        self.on_mutation_success(None, text);
        // Drop selected paths that no longer resolve — deleted nodes must not
        // leave a dead selection that silently blocks the next operation
        // (copy/remark/paste all read `selected_paths()`). Co-selected paths
        // that still resolve are kept; positional paths below a deleted
        // sibling may shift index and are dropped as unresolvable.
        let rows = self.visible_rows();
        let alive: Vec<Path> = self
            .selection
            .iter()
            .filter(|p| rows.iter().any(|r| r.path == **p))
            .collect();
        // Snap the cursor back to where the deletion started. `first_idx` is a
        // pre-delete row index, so clamp it to the shortened list; deleting the
        // tail lands on the new last row, and deleting everything leaves the
        // empty-document cursor `compute_rows` already set.
        if cursor_deleted && !rows.is_empty() {
            if let Some(i) = first_idx {
                self.cursor = rows[i.min(rows.len() - 1)].path.clone();
            }
        }
        if alive.is_empty() {
            self.selection.clear();
            self.last_action_was_shift_select = false;
        } else {
            self.selection.set_all(alive);
        }
    }

    pub fn copy_selected(&mut self) {
        self.capture_selected(false);
    }

    pub fn cut_selected(&mut self) {
        self.capture_selected(true);
    }

    /// Shared copy/cut capture. `cut` selects the clipboard mode, the toggle
    /// message, and (cut only) the read-only guard.
    fn capture_selected(&mut self, cut: bool) {
        if cut && self.cursor_is_read_only() {
            self.set_notice(Notice::core(self.lang, "core.readonly", &[]));
            return;
        }
        if let Some(cb) = &mut self.clipboard {
            if cb.cut != cut {
                cb.cut = cut;
                let n = cb.fragments.len().to_string();
                let key = if cut {
                    "core.clipboard.cut-changed"
                } else {
                    "core.clipboard.copied-changed"
                };
                self.set_notice(Notice::core(self.lang, key, &[&n]));
            }
            return;
        }
        let paths = self.selected_paths();
        if paths.is_empty() {
            return;
        }
        let doc = match self.doc.as_ref() {
            Some(d) => d,
            None => return,
        };
        let mut fragments = Vec::new();
        for p in &paths {
            fragments.push(doc.serialize_fragment_relative(p));
        }
        self.clipboard = Some(Clipboard {
            fragments,
            cut,
            sources: paths,
        });
        self.paste_slot = None;
        let n = self.clipboard.as_ref().unwrap().fragments.len().to_string();
        let key = if cut {
            "core.clipboard.cut"
        } else {
            "core.clipboard.copied"
        };
        self.set_notice(Notice::core(self.lang, key, &[&n]));
    }

    pub fn paste(&mut self) {
        let cb = match self.clipboard.take() {
            Some(cb) => cb,
            None => {
                self.set_notice(Notice::core(self.lang, "core.clipboard.empty", &[]));
                return;
            }
        };
        let target = match self.slot_target(self.effective_paste_slot()) {
            Some(t) => t,
            None => {
                self.clipboard = Some(cb);
                return;
            }
        };
        self.do_paste(cb, target, OnCollision::Cancel, false);
    }

    /// Drag-reparent / drag-reorder (Web mouse grip, touch grip): move (or,
    /// with `cut: false`, copy) `sources` to `slot`. Implemented as a one-shot
    /// cut/copy→paste so it reuses `do_paste`'s entire collision /
    /// illegal-destination / array-upgrade machinery (a real
    /// `Mutation::Move`/`Insert` under the hood).
    ///
    /// The destination arrives as a `PasteSlot` and is resolved by the same
    /// `slot_target` a keyboard `Paste` uses (ADR 0010) — *not* as a
    /// host-computed `parent`/`index`. That equality is the point: pointer
    /// hosts used to hand-roll "before/after this row means a sibling in its
    /// parent", which contradicted `resolve_target`'s rule that `After` an
    /// *expanded* branch means that branch's **first child**, so dragging into
    /// the gap under an expanded `[table]` aimed one level too shallow (in
    /// TOML, usually straight into a `core.paste.error`) while an armed paste
    /// released at the very same pixel landed inside. A drop onto a source or
    /// into its own subtree is rejected; the document is untouched on any
    /// failure, and a slot whose row is no longer visible is ignored.
    pub fn move_selection_to(&mut self, sources: Vec<Path>, slot: PasteSlot, cut: bool) {
        if self.guard_clipboard_locked() {
            return;
        }
        if self.doc.is_none() {
            return;
        }
        let sources = crate::session::selection::normalize(sources);
        if sources.is_empty() {
            return;
        }
        // One resolution point, shared with `paste()` — a stale slot (row
        // collapsed/scrolled away between the drop and dispatch) is dropped
        // silently, like `set_paste_slot`'s visibility guard.
        let Some(tgt) = self.slot_target(slot) else {
            return;
        };
        if sources
            .iter()
            .any(|s| tgt.parent == *s || (tgt.parent.len() > s.len() && tgt.parent.starts_with(s)))
        {
            self.set_notice(Notice::core(self.lang, "core.move.self", &[]));
            return;
        }
        let Some(doc) = self.doc.as_ref() else {
            return;
        };
        let fragments: Vec<String> = sources
            .iter()
            .map(|p| doc.serialize_fragment_relative(p))
            .collect();
        let cb = Clipboard {
            fragments,
            cut,
            sources,
        };
        // `do_paste`'s failure contract restores its clipboard — but this one was
        // synthesized for the drag (cut:true), so a failed drop would leave the UI
        // armed in paste-cut mode. Restore whatever the user had armed instead,
        // unless a prompt (collision / array-upgrade) is pending and still needs
        // the drag fragments to complete.
        let prev = self.clipboard.take();
        self.do_paste(cb, tgt, OnCollision::Cancel, false);
        if matches!(self.mode, Mode::Normal) {
            self.clipboard = prev;
        }
    }

    pub fn do_paste(
        &mut self,
        clipboard: Clipboard,
        target: Target,
        on_collision: OnCollision,
        allow_upgrade: bool,
    ) {
        let Clipboard {
            fragments,
            cut: is_cut,
            sources,
        } = clipboard;
        let is_comment = |p: &Path| {
            self.tree
                .node_at(p)
                .map(|n| matches!(n.kind, NodeKind::Comment(_)))
                .unwrap_or(false)
        };
        let mut node_entries: Vec<(String, Path)> = Vec::new();
        let mut comment_entries: Vec<(String, Path)> = Vec::new();
        // `sources` may be shorter than `fragments` (e.g. a paste whose source
        // paths weren't captured); missing entries pad with an empty path.
        let mut srcs = sources.into_iter();
        for frag in fragments {
            let src = srcs.next().unwrap_or_default();
            if is_comment(&src) {
                comment_entries.push((frag, src));
            } else {
                node_entries.push((frag, src));
            }
        }
        let rebuild =
            |is_cut: bool, nodes: &[(String, Path)], comments: &[(String, Path)]| -> Clipboard {
                let mut fragments = Vec::new();
                let mut sources = Vec::new();
                for (f, s) in nodes.iter().chain(comments.iter()) {
                    fragments.push(f.clone());
                    sources.push(s.clone());
                }
                Clipboard {
                    fragments,
                    cut: is_cut,
                    sources,
                }
            };
        if self.doc.is_none() {
            self.clipboard = Some(rebuild(is_cut, &node_entries, &comment_entries));
            return;
        }
        if !comment_entries.is_empty() {
            enum Dest {
                Ok,
                Prompt,
                Illegal,
            }
            let dest = self
                .tree
                .node_at(&target.parent)
                .map(|n| match n.kind {
                    NodeKind::Root | NodeKind::Table => Dest::Ok,
                    NodeKind::Array if n.value.is_none() => Dest::Ok,
                    NodeKind::Array if allow_upgrade => Dest::Ok,
                    NodeKind::Array => Dest::Prompt,
                    _ => Dest::Illegal,
                })
                .unwrap_or(Dest::Illegal);
            match dest {
                Dest::Ok => {}
                Dest::Prompt => {
                    self.clipboard = Some(rebuild(is_cut, &node_entries, &comment_entries));
                    self.mode = Mode::Prompt(PromptKind::ArrayUpgrade {
                        target,
                        on_collision,
                    });
                    return;
                }
                Dest::Illegal => {
                    self.clipboard = Some(rebuild(is_cut, &node_entries, &comment_entries));
                    self.set_notice(Notice::core(self.lang, "core.paste.comment-illegal", &[]));
                    return;
                }
            }
        }
        let mut text = self.doc.as_ref().map(|d| d.serialize()).unwrap_or_default();
        // ---- NODE PHASE ----
        if is_cut {
            let node_sources: Vec<Path> = node_entries.iter().map(|(_, s)| s.clone()).collect();
            if !node_sources.is_empty() {
                let doc = self.doc.as_mut().unwrap();
                match doc.apply(Mutation::Move {
                    sources: node_sources,
                    target: target.clone(),
                    on_collision,
                }) {
                    Ok(t) => text = t,
                    Err(MutateError::Collision(key)) => {
                        self.clipboard = Some(rebuild(is_cut, &node_entries, &comment_entries));
                        self.mode = Mode::Prompt(PromptKind::Collision { key });
                        return;
                    }
                    Err(e) => {
                        self.clipboard = Some(rebuild(is_cut, &node_entries, &comment_entries));
                        self.set_notice(Notice::core(
                            self.lang,
                            "core.paste.error",
                            &[&e.to_string()],
                        ));
                        return;
                    }
                }
            }
        } else {
            let dest_packs = self
                .tree
                .node_at(&target.parent)
                .map(|n| matches!(n.kind, NodeKind::ArrayOfTables | NodeKind::Array))
                .unwrap_or(false);
            // Each grouped fragment keeps its source `Path`: a bare-scalar
            // array element being copied out of a keyed array carries no key
            // of its own, and the insert below derives `<arrayKey>_<index>`
            // from that path — the same name the Move (cut) path gives it.
            // The packed/joined branch can't map back to a single source, but
            // it only fires for array/AoT destinations, where key synthesis
            // never happens, so `None` is harmless there.
            let grouped: Vec<(String, usize, Option<Path>)> = if dest_packs
                && node_entries.len() > 1
                && node_entries
                    .iter()
                    .all(|(f, _)| crate::model::cst_edit::joinable_entry(f))
            {
                let joined: String = node_entries.iter().map(|(f, _)| f.as_str()).collect();
                vec![(joined, 0, None)]
            } else {
                node_entries
                    .iter()
                    .enumerate()
                    .map(|(i, (f, s))| (f.clone(), i, Some(s.clone())))
                    .collect()
            };
            for (frag, i, src) in &grouped {
                let i = *i;
                let doc = self.doc.as_mut().unwrap();
                match doc.apply(Mutation::Insert {
                    target: target.clone(),
                    fragment: frag.clone(),
                    on_collision,
                    // `None` for anything but a bare scalar pulled from a
                    // keyed array: keyed fragments ignore this field, and
                    // unkeyed/nested arrays fall back to the format's
                    // generic placeholder key, as before this existed.
                    suggested_key: src
                        .as_deref()
                        .and_then(crate::model::node::array_element_suggested_key),
                }) {
                    Ok(t) => text = t,
                    Err(MutateError::Collision(key)) => {
                        // Earlier entries in this loop may have already mutated
                        // `self.doc` — re-project `self.tree` before returning so it
                        // doesn't go stale relative to the real document (the same
                        // reason the comment phase below already does this).
                        self.on_mutation_success(None, text);
                        self.clipboard =
                            Some(rebuild(is_cut, &node_entries[i..], &comment_entries));
                        self.mode = Mode::Prompt(PromptKind::Collision { key });
                        return;
                    }
                    Err(e) => {
                        self.on_mutation_success(None, text);
                        self.clipboard =
                            Some(rebuild(is_cut, &node_entries[i..], &comment_entries));
                        self.set_notice(Notice::core(
                            self.lang,
                            "core.paste.error",
                            &[&e.to_string()],
                        ));
                        return;
                    }
                }
            }
        }
        // ---- COMMENT PHASE ----
        let orig_ord = |p: &Path| -> Option<usize> {
            self.tree
                .node_at(&target.parent)
                .and_then(|par| par.children.iter().position(|c| &c.path == p))
        };
        let node_shift = if is_cut {
            node_entries
                .iter()
                .filter(|(_, s)| orig_ord(s).is_some_and(|o| o < target.index))
                .count()
        } else {
            0
        };
        let comment_ords: Vec<Option<usize>> =
            comment_entries.iter().map(|(_, s)| orig_ord(s)).collect();
        let n_comments = comment_entries.len();
        for rev in 0..n_comments {
            let oi = n_comments - 1 - rev;
            let (frag, src) = &comment_entries[oi];
            let comment_shift = if is_cut {
                comment_ords[oi..]
                    .iter()
                    .filter(|o| o.is_some_and(|o| o < target.index))
                    .count()
            } else {
                0
            };
            let ctarget = Target {
                parent: target.parent.clone(),
                index: target.index.saturating_sub(node_shift + comment_shift),
            };
            if is_cut {
                let doc = self.doc.as_mut().unwrap();
                match doc.apply(Mutation::Delete { path: src.clone() }) {
                    Ok(t) => text = t,
                    Err(e) => {
                        self.on_mutation_success(None, text);
                        self.clipboard = Some(rebuild(is_cut, &[], &comment_entries[..=oi]));
                        self.set_notice(Notice::core(
                            self.lang,
                            "core.paste.error",
                            &[&e.to_string()],
                        ));
                        return;
                    }
                }
            }
            let doc = self.doc.as_mut().unwrap();
            match doc.apply(Mutation::InsertComment {
                target: ctarget.clone(),
                text: frag.clone(),
            }) {
                Ok(t) => text = t,
                Err(e) => {
                    let end = if is_cut { oi } else { oi + 1 };
                    self.on_mutation_success(None, text);
                    self.clipboard = Some(rebuild(is_cut, &[], &comment_entries[..end]));
                    self.set_notice(Notice::core(
                        self.lang,
                        "core.paste.error",
                        &[&e.to_string()],
                    ));
                    return;
                }
            }
        }
        self.on_mutation_success(None, text);
        // Drop the source selection and clear any leftover selection, then move
        // the cursor onto the first freshly-pasted/moved node so it's clearly
        // visible -- expanding every collapsed ancestor of the destination, not
        // just its immediate parent, so a deeply-nested target surfaces
        // correctly too (mirrors `reveal_path`'s prefix-expansion). Deliberately
        // does NOT select the pasted/moved node(s): `self.selection` is a
        // persistent, opt-in multi-select the user builds explicitly (`s` /
        // Shift-range) that must survive plain cursor movement -- if paste/move
        // populated it, the very next unrelated `cursor_down` + `c` would
        // silently re-copy the old paste/move instead of the node now under the
        // cursor. They land contiguously starting at `target.index - shift`: on
        // a same-parent cut, every source (node *or* comment) that sat above the
        // target was removed first, shifting the landing slot up by that count
        // (the Move/Insert/InsertComment mutations already account for it, so
        // the cursor must too -- else a downward move lands on the next row).
        // `node_shift` covers the nodes; the comment sources above the target
        // add the rest.
        let comment_shift = if is_cut {
            comment_ords
                .iter()
                .filter(|o| o.is_some_and(|o| o < target.index))
                .count()
        } else {
            0
        };
        let pasted = node_entries.len() + comment_entries.len();
        for i in 0..=target.parent.len() {
            self.expanded.insert(target.parent[..i].to_vec());
        }
        self.selection.clear();
        if let Some(parent) = self.tree.node_at(&target.parent) {
            let n = parent.children.len();
            if pasted > 0 && n > 0 {
                let start = target
                    .index
                    .saturating_sub(node_shift + comment_shift)
                    .min(n - 1);
                if let Some(first) = parent.children.get(start).map(|c| c.path.clone()) {
                    self.cursor = first;
                }
            }
        }
    }

    pub fn remark(&mut self) {
        if self.guard_clipboard_locked() {
            return;
        }
        if self.cursor_is_read_only() {
            self.set_notice(Notice::core(self.lang, "core.readonly", &[]));
            return;
        }
        let paths = self.selected_paths();
        if paths.is_empty() {
            return;
        }
        // Same contract as `delete_selected`: an active multi-select wins
        // over the cursor. Process in top-down row order: each remark can
        // then only merge into a block ABOVE it (never invalidate an already
        // recorded post-image below), and `normalize()` has already removed
        // any ancestor/descendant pairs, so depth order is irrelevant.
        let had_selection = !self.selection.is_empty();
        let rows = self.visible_rows();
        let mut targets: Vec<Path> = paths
            .into_iter()
            .filter(|p| rows.iter().any(|r| &r.path == p))
            .collect();
        targets.sort_by_key(|p| rows.iter().position(|r| &r.path == p));
        let mut remapped: Vec<(usize, Path)> = Vec::new();
        for p in &targets {
            let rows = self.visible_rows();
            let Some(idx) = rows.iter().position(|r| r.path == *p) else {
                continue; // already-stale selected path: drop it, self-heal
            };
            let is_branch = rows[idx].is_branch;
            // A comment row: the projected tree node's kind is Comment (the
            // row's `value` carries the comment text, so it can't be used).
            let above_is_comment = idx > 0
                && self
                    .tree
                    .node_at(&rows[idx - 1].path)
                    .is_some_and(|n| matches!(n.kind, NodeKind::Comment(_)));
            let n_before = rows.len();
            self.do_remark(p.clone());
            if !had_selection {
                continue; // plain cursor remark: do_remark's re-anchor suffices
            }
            // Remap the selection onto the remark's post-image so it survives
            // the three shapes a remark can take: in-place kind swap
            // (Key<->Index, row count unchanged), merge of adjacent comment
            // rows into one block (count shrinks — an adjacent block absorbs
            // the row), and un-remark splitting a block back into 1+d live
            // rows (count grows).
            let rows = self.visible_rows();
            let delta = rows.len() as isize - n_before as isize;
            let clamp = |i: usize| i.min(rows.len().saturating_sub(1));
            if delta > 0 {
                for (off, r) in rows.iter().skip(idx).take(1 + delta as usize).enumerate() {
                    remapped.push((idx + off, r.path.clone()));
                }
            } else if delta < 0 && !is_branch && above_is_comment {
                // leaf merged into the comment block right above it
                let i = clamp(idx.saturating_sub(1));
                remapped.push((i, rows[i].path.clone()));
            } else if let Some(r) = rows.get(clamp(idx)) {
                // in-place swap, container collapse, or merge-down
                remapped.push((clamp(idx), r.path.clone()));
            }
        }
        if remapped.is_empty() {
            if !self.selection.is_empty() {
                // nothing survived remapping — a dead selection must not
                // silently block the next operation
                self.selection.clear();
                self.last_action_was_shift_select = false;
            }
            return;
        }
        remapped.sort_by_key(|(i, _)| *i);
        remapped.dedup_by(|a, b| a.0 == b.0);
        let paths: Vec<Path> = remapped.into_iter().map(|(_, p)| p).collect();
        self.selection.set_all(paths.iter().cloned());
        // Anchor the cursor on the topmost remapped row (mirrors paste
        // anchoring the cursor on the first pasted node).
        if let Some(r) = self.visible_rows().iter().find(|r| r.path == paths[0]) {
            self.cursor = r.path.clone();
        }
    }

    pub(crate) fn do_remark(&mut self, path: Path) {
        // Remark toggles the cursor row's own kind (Node<->Comment) in place
        // without moving it, but its *addressing* changes (a keyed path becomes
        // positional, or vice versa) — the stale `self.cursor` path no longer
        // resolves post-mutation, so `compute_rows`'s "snap to first row when
        // cursor path vanished" fallback fires and the cursor jumps to row 0.
        // Capture the row's visible *index* (unaffected by the kind swap, since
        // no sibling above it moves) and re-anchor `self.cursor` by index below.
        let row_index = self.cursor_row_index();
        let doc = match self.doc.as_mut() {
            Some(d) => d,
            None => return,
        };
        match doc.apply(Mutation::Remark { path }) {
            Ok(text) => {
                self.on_mutation_success(None, text);
                if let Some(row) = row_index.and_then(|i| self.visible_rows().get(i).cloned()) {
                    self.cursor = row.path;
                }
            }
            Err(MutateError::Fragment(_)) => {
                self.set_notice(Notice::core(self.lang, "core.remark.invalid", &[]));
            }
            Err(e) => self.set_notice(Notice::core(
                self.lang,
                "core.remark.error",
                &[&e.to_string()],
            )),
        }
    }
}
