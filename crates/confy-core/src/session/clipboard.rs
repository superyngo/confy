//! Cut/copy/paste + the paste collision/array-upgrade prompt sub-state-
//! machine — split out of `session.rs` (Task 15, 2026-08-11 audit
//! remediation).

use crate::model::document::{ConfigDocument, MutateError, Mutation, OnCollision, Target};
use crate::model::node::{NodeKind, Path};
use crate::session::i18n::{tr, tr_args};
use crate::session::state::{Clipboard, Mode, PendingComment, PromptKind};

use super::session::Session;

impl Session {
    pub fn delete_selected(&mut self) {
        if self.cursor_is_read_only() {
            self.status = Some(tr(self.lang, "core.readonly").to_string());
            return;
        }
        let paths = self.selected_paths();
        if paths.is_empty() {
            return;
        }
        let mut paths = paths;
        paths.sort_by_key(|b| std::cmp::Reverse(b.len()));
        let doc = match self.doc.as_mut() {
            Some(d) => d,
            None => return,
        };
        for p in &paths {
            if let Err(e) = doc.apply(Mutation::Delete { path: p.clone() }) {
                self.error = Some(tr_args(self.lang, "core.delete.error", &[&e.to_string()]));
                return;
            }
        }
        self.on_mutation_success(None);
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
            self.status = Some(tr(self.lang, "core.readonly").to_string());
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
                self.status = Some(tr_args(self.lang, key, &[&n]));
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
        self.status = Some(tr_args(self.lang, key, &[&n]));
    }

    pub fn paste(&mut self) {
        let cb = match self.clipboard.take() {
            Some(cb) => cb,
            None => {
                self.status = Some(tr(self.lang, "core.clipboard.empty").to_string());
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

    /// Drag-reparent (Web UI): move `sources` into `target` at child `index`.
    /// Implemented as a one-shot cut→paste so it reuses `do_paste`'s entire
    /// collision / illegal-destination / array-upgrade machinery (a real
    /// `Mutation::Move` under the hood). A drop onto a source or into its own
    /// subtree is rejected; the document is untouched on any failure.
    pub fn move_selection_to(&mut self, sources: Vec<Path>, target: Path, index: usize) {
        if self.doc.is_none() {
            return;
        }
        let sources = crate::session::selection::normalize(sources);
        if sources.is_empty() {
            return;
        }
        if sources
            .iter()
            .any(|s| target == *s || (target.len() > s.len() && target.starts_with(s)))
        {
            self.error = Some(tr(self.lang, "core.move.self").to_string());
            return;
        }
        let doc = self.doc.as_ref().unwrap();
        let fragments: Vec<String> = sources
            .iter()
            .map(|p| doc.serialize_fragment_relative(p))
            .collect();
        let cb = Clipboard {
            fragments,
            cut: true,
            sources,
        };
        let tgt = Target {
            parent: target,
            index,
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
                    self.status =
                        Some(tr(self.lang, "core.paste.array-upgrade-confirm").to_string());
                    self.mode = Mode::Prompt(PromptKind::ArrayUpgrade {
                        target,
                        on_collision,
                    });
                    return;
                }
                Dest::Illegal => {
                    self.clipboard = Some(rebuild(is_cut, &node_entries, &comment_entries));
                    self.error = Some(tr(self.lang, "core.paste.comment-illegal").to_string());
                    return;
                }
            }
        }
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
                    Ok(()) => {}
                    Err(MutateError::Collision(key)) => {
                        self.clipboard = Some(rebuild(is_cut, &node_entries, &comment_entries));
                        self.error = Some(tr_args(self.lang, "core.paste.collision", &[&key]));
                        self.mode = Mode::Prompt(PromptKind::Collision { key });
                        return;
                    }
                    Err(e) => {
                        self.clipboard = Some(rebuild(is_cut, &node_entries, &comment_entries));
                        self.error =
                            Some(tr_args(self.lang, "core.paste.error", &[&e.to_string()]));
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
            let grouped: Vec<(String, usize)> = if dest_packs
                && node_entries.len() > 1
                && node_entries
                    .iter()
                    .all(|(f, _)| crate::model::cst_edit::joinable_entry(f))
            {
                let joined: String = node_entries.iter().map(|(f, _)| f.as_str()).collect();
                vec![(joined, 0)]
            } else {
                node_entries
                    .iter()
                    .enumerate()
                    .map(|(i, (f, _))| (f.clone(), i))
                    .collect()
            };
            let doc = self.doc.as_mut().unwrap();
            for (frag, i) in &grouped {
                let i = *i;
                match doc.apply(Mutation::Insert {
                    target: target.clone(),
                    fragment: frag.clone(),
                    on_collision,
                }) {
                    Ok(()) => {}
                    Err(MutateError::Collision(key)) => {
                        self.clipboard =
                            Some(rebuild(is_cut, &node_entries[i..], &comment_entries));
                        self.error = Some(tr_args(self.lang, "core.paste.collision", &[&key]));
                        self.mode = Mode::Prompt(PromptKind::Collision { key });
                        return;
                    }
                    Err(e) => {
                        self.clipboard =
                            Some(rebuild(is_cut, &node_entries[i..], &comment_entries));
                        self.error =
                            Some(tr_args(self.lang, "core.paste.error", &[&e.to_string()]));
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
                if let Err(e) = doc.apply(Mutation::Delete { path: src.clone() }) {
                    self.on_mutation_success(None);
                    self.clipboard = Some(rebuild(is_cut, &[], &comment_entries[..=oi]));
                    self.error = Some(tr_args(self.lang, "core.paste.error", &[&e.to_string()]));
                    return;
                }
            }
            let doc = self.doc.as_mut().unwrap();
            if let Err(e) = doc.apply(Mutation::InsertComment {
                target: ctarget.clone(),
                text: frag.clone(),
            }) {
                let end = if is_cut { oi } else { oi + 1 };
                self.on_mutation_success(None);
                self.clipboard = Some(rebuild(is_cut, &[], &comment_entries[..end]));
                self.error = Some(tr_args(self.lang, "core.paste.error", &[&e.to_string()]));
                return;
            }
        }
        self.on_mutation_success(None);
        // Drop the source selection and move both cursor and selection onto the
        // freshly-pasted node(s). They land contiguously starting at
        // `target.index - shift`: on a same-parent cut, every source (node *or*
        // comment) that sat above the target was removed first, shifting the
        // landing slot up by that count (the Move/Insert/InsertComment mutations
        // already account for it, so the selection must too — else a downward
        // move selects the next row). `node_shift` covers the nodes; the comment
        // sources above the target add the rest.
        let comment_shift = if is_cut {
            comment_ords
                .iter()
                .filter(|o| o.is_some_and(|o| o < target.index))
                .count()
        } else {
            0
        };
        let pasted = node_entries.len() + comment_entries.len();
        if let Some(parent) = self.tree.node_at(&target.parent) {
            let n = parent.children.len();
            if pasted > 0 && n > 0 {
                let start = target
                    .index
                    .saturating_sub(node_shift + comment_shift)
                    .min(n - 1);
                let end = (start + pasted).min(n);
                let paths: Vec<Path> = parent.children[start..end]
                    .iter()
                    .map(|c| c.path.clone())
                    .collect();
                if let Some(first) = paths.first().cloned() {
                    self.selection.set_all(paths);
                    self.cursor = first;
                }
            }
        }
    }

    pub fn remark(&mut self) {
        if self.cursor_is_read_only() {
            self.status = Some(tr(self.lang, "core.readonly").to_string());
            return;
        }
        let path = match self.cursor_row() {
            Some(r) => r.path,
            None => return,
        };
        let authoring = self
            .tree
            .node_at(&path)
            .map(|n| !matches!(n.kind, NodeKind::Comment(_)))
            .unwrap_or(false);
        let supports = self
            .doc
            .as_ref()
            .map(|d| d.supports_comments())
            .unwrap_or(true);
        if authoring && !supports {
            self.mode = Mode::Prompt(PromptKind::JsoncUpgrade {
                pending: PendingComment::Remark { path },
            });
            return;
        }
        self.do_remark(path);
    }

    pub(crate) fn do_remark(&mut self, path: Path) {
        let doc = match self.doc.as_mut() {
            Some(d) => d,
            None => return,
        };
        match doc.apply(Mutation::Remark { path }) {
            Ok(()) => self.on_mutation_success(None),
            Err(MutateError::Fragment(_)) => {
                self.status = Some(tr(self.lang, "core.remark.invalid").to_string());
            }
            Err(e) => self.error = Some(tr_args(self.lang, "core.remark.error", &[&e.to_string()])),
        }
    }
}
