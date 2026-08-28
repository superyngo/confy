# JSON/JSONC comment write-gate removal Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove the JSON/JSONC write-permission gate (`JsonDocument.comments_enabled` / `ConfigDocument::supports_comments()` / the `PromptKind::JsoncUpgrade` interactive prompt) across `confy-core`, `confy-ffi`, `confy-tui`, and `web`, so authoring a comment into any `.json` document is always mechanically legal, matching TOML/YAML. A new content-only fact, `had_comments_at_open()`, replaces `supports_comments()` as the sole reader for the existing one-shot "file already had comments" load-time toast — this is a rewire, not a new behavior.

**Architecture:** Bottom-up removal: `confy-core` model layer (Task 1) → `confy-core` session layer (Task 2) → `confy-ffi` wasm binding (Task 3) → `confy-tui` (Task 4) → `web` (Task 5) → whole-workspace grep+build verification (Task 6). Each task leaves the workspace in a compiling, test-passing state.

**Tech Stack:** Rust (confy-core, confy-tui, confy-ffi crates; `cargo test`/`cargo build`), TypeScript (web/, no build step needed for verification — `tsc`-checked implicitly by existing tooling if run, but this plan's verification only requires the Rust side to compile/test since `confy-ffi`'s wasm output is what web imports), `wasm-pack` for the FFI crate.

**Spec:** `docs/superpowers/specs/2026-08-28-json-jsonc-comment-gate-removal-design.md`

## Global Constraints

- Every deletion is a clean cutover — no dead code, no compatibility aliases, no `#[deprecated]` shims.
- No new one-shot toast is added for authoring a comment mid-session; `comment_advisory` (unchanged, per-row projection) is the sole signal for that case.
- The existing one-shot `*.host.json-comments-detected` load-time toast keeps identical observed behavior — it fires iff the file already contained a comment when opened, never for a comment authored mid-session.
- `strict_json` and `comment_advisory` are untouched by this plan (spec "What stays unchanged").
- Final state has zero references anywhere in the workspace to: `comments_enabled`, `enable_comments`, `PromptKind::JsoncUpgrade`, `PendingComment`, `core.comment.unsupported`, `supports_comments`/`supportsComments` (verified by grep in Task 6, not just compiler silence).

## Execution Status

**Executed 2026-08-28, inline (no subagent dispatch), all 6 tasks complete.** See commits
`ccfdfbb`..`cae12ad` on `main`. Two additional stray references not anticipated by this plan's
investigation were found and fixed during execution (both consistent with the plan's own
contingency note): `crates/confy-core/src/session/mod.rs`'s `PendingComment` re-export (Task 2),
and `crates/confy-tui/src/tui/state.rs`'s `PendingComment` re-export (Task 4). Task 2's two new
regression tests (`add_comment_sibling_never_blocked_on_clean_json`,
`remark_never_prompts_on_clean_json`) were written directly against `Session::dispatch(Intent::…)`
in `tests/session_headless.rs` rather than the plan's guessed lower-level `Session` API, since
`add_comment_sibling` is a private method only reachable via `Intent::AddSibling` when the cursor
already sits on a comment-kind row — the test first calls `Intent::Remark` to create that row
in-session on an otherwise-clean `.json`, then adds a sibling next to it. All steps verified: full
`cargo test --workspace` (0 failures), `wasm-pack build --target web`, `web`'s `tsc --noEmit` +
`npm test` (0 failures), the VS Code extension's `tsc --noEmit`, and a live `confy-tui` run
against both a pre-commented and a clean `.json` scratch file confirming the toast/no-prompt
behavior on the real binary.

---

### Task 1: `confy-core` model layer — drop `supports_comments()`, add `had_comments_at_open()`

**Files:**
- Modify: `crates/confy-core/src/model/document.rs:27-29` (trait declaration)
- Modify: `crates/confy-core/src/model/json/doc.rs` (`JsonDocument` — field, impl, `from_str`, `enable_comments`, tests)
- Modify: `crates/confy-core/src/model/any_doc.rs` (`AnyDocument` — `enable_comments`, delegating impl, tests)
- Modify: `crates/confy-core/src/model/cst_doc.rs:78-80` (`TomlDocument`, plus test at `:410-415`)
- Modify: `crates/confy-core/src/model/yaml/doc.rs:63-65` (`YamlDocument`, plus test at `:271-280`)
- Test: existing unit tests in each of the above files (rewritten, not new files)

**Interfaces:**
- Consumes: nothing from other tasks (this is the base layer).
- Produces: `ConfigDocument::had_comments_at_open(&self) -> bool` (new trait method, default body `false`); `JsonDocument` overrides it to return its stored fact. `ConfigDocument::supports_comments` no longer exists — Task 2 (session layer) is the consumer that must stop calling it.

- [x] **Step 1: Delete `ConfigDocument::supports_comments` from the trait, add `had_comments_at_open`**

In `crates/confy-core/src/model/document.rs`, replace lines 27-29:

```rust
    // DELETE:
    // /// Whether authored comments are currently legal in this document
    // /// (false only for a pure `.json` before the JSONC upgrade, Phase 2).
    // fn supports_comments(&self) -> bool;
```

with:

```rust
    /// Whether this document already contained an authored comment when it was
    /// loaded (content-derived, fixed at construction — never a write
    /// permission). Used only to drive the one-shot "file already had
    /// comments" load-time toast. Defaults `false`; only `JsonDocument`
    /// overrides it, since TOML/YAML have no "this format silently accepted a
    /// foreign notation" surprise to report.
    fn had_comments_at_open(&self) -> bool {
        false
    }
```

- [x] **Step 2: Update `JsonDocument` — field, `from_str`, delete `enable_comments`**

In `crates/confy-core/src/model/json/doc.rs`, replace the struct field block (current lines 17-21):

```rust
    // DELETE:
    // /// True once authored comments are legal: the file already contained a `//`
    // /// or `/* */` at load, OR the host enabled it for a `.jsonc` extension, OR the
    // /// user accepted the JSONC upgrade this session. A pure `.json` with no
    // /// comments starts false.
    // pub(crate) comments_enabled: bool,
```

with:

```rust
    /// True iff the file already contained a `//` or `/* */` comment when it
    /// was loaded (content-derived at `from_str`, never written after
    /// construction — comments are always legal to author, so there is
    /// nothing left to "enable").
    pub(crate) had_comments_at_open: bool,
```

Replace the `supports_comments` impl (current lines 66-68):

```rust
    // DELETE:
    // fn supports_comments(&self) -> bool {
    //     self.comments_enabled
    // }
```

with:

```rust
    fn had_comments_at_open(&self) -> bool {
        self.had_comments_at_open
    }
```

In `from_str` (current lines 144-163), rename the local binding and drop the doc-comment's `enable_comments` cross-reference. Replace the whole function:

```rust
    /// Parse a document from in-memory text (no file system). `had_comments_at_open`
    /// is derived from content only (a `//` or `/* */` present at load).
    /// The projection root label (`filename`) starts empty; the host sets it via
    /// [`set_filename`](Self::set_filename).
    #[allow(clippy::should_implement_trait)] // named per PORTING.md; see cst_doc.rs
    pub fn from_str(text: &str) -> anyhow::Result<Self> {
        let green = crate::model::json::parse::parse(text)
            .map_err(|e| anyhow::anyhow!("parsing JSON: {e}"))?;
        // Derived from the token stream, not raw text, so a `//` inside a string
        // value does not count as a comment.
        let had_comments_at_open = crate::model::json::parse::lex(text).iter().any(|(k, _)| {
            matches!(
                k,
                crate::model::json::syntax::SyntaxKind::LINE_COMMENT
                    | crate::model::json::syntax::SyntaxKind::BLOCK_COMMENT
            )
        });
        Ok(JsonDocument {
            syntax: SyntaxNode::new_root(green),
            original: text.to_string(),
            clean: true,
            filename: String::new(),
            had_comments_at_open,
        })
    }
```

Delete the `enable_comments` method entirely (current lines 182-185):

```rust
    // DELETE:
    // /// Accept the JSONC upgrade: authored comments become legal for this session.
    // pub fn enable_comments(&mut self) {
    //     self.comments_enabled = true;
    // }
```

- [x] **Step 3: Rewrite `JsonDocument`'s test module**

In `crates/confy-core/src/model/json/doc.rs`'s `#[cfg(test)] mod tests` (current lines 220-383), the helper `json_from_str` currently calls `doc.enable_comments()` for a `.jsonc` extension — that path no longer exists (extension-driven enabling is gone; content is the only trigger). Replace the helper and every affected test. Full replacement of lines 225-269 and 345-351:

```rust
    /// Parse `s`. The `ext` parameter is now unused (extension no longer
    /// drives any comment-related fact) but kept so call sites read the same;
    /// tests exercising the old `.jsonc`-extension trigger are deleted below
    /// since that trigger no longer exists.
    fn json_from_str(_ext: &str, s: &str) -> JsonDocument {
        JsonDocument::from_str(s).unwrap()
    }

    #[test]
    fn roundtrip_and_facets() {
        let src = "{\n  \"a\": 1\n}\n";
        let doc = json_from_str(".json", src);
        assert_eq!(doc.serialize(), src);
        assert!(!doc.is_dirty());
        assert_eq!(doc.format(), DocFormat::Json);
        assert_eq!(doc.comment_prefix(), "//");
    }

    #[test]
    fn pure_json_has_no_comments_at_open() {
        let doc = json_from_str(".json", "{}\n");
        assert!(!doc.had_comments_at_open());
    }

    #[test]
    fn existing_comment_sets_had_comments_at_open() {
        let doc = json_from_str(".json", "// hi\n{}\n");
        assert!(doc.had_comments_at_open());
    }

    #[test]
    fn slashes_inside_string_do_not_set_had_comments_at_open() {
        let doc = json_from_str(".json", "{\n  \"url\": \"https://a.com\"\n}\n");
        assert!(!doc.had_comments_at_open());
        let doc = json_from_str(".json", "{\n  \"glob\": \"/* not a comment */\"\n}\n");
        assert!(!doc.had_comments_at_open());
    }
```

(`jsonc_extension_supports_comments`, current lines 251-255, is deleted outright — it tested extension-driven write permission, a concept that no longer exists.)

Later in the same module, replace `enable_comments_then_supports` (current lines 345-351) — it tested the write-permission toggle, which no longer exists — by deleting it entirely (no replacement; nothing about `had_comments_at_open` changes after construction, so there's nothing to assert post-construction).

Also update the doc-comment reference on line 225 (`.jsonc`-extension comment-enable... exercise that path without touching the fs) since that comment is now stale — it's covered by the new helper doc-comment above.

- [x] **Step 4: Update `AnyDocument` — delete `enable_comments`, update delegate + test**

In `crates/confy-core/src/model/any_doc.rs`, delete the inherent method (current lines 66-71):

```rust
    // DELETE:
    // /// Accept the JSONC upgrade (enables authored comments). No-op for TOML.
    // pub fn enable_comments(&mut self) {
    //     if let AnyDocument::Json(d) = self {
    //         d.enable_comments();
    //     }
    // }
```

In the `impl ConfigDocument for AnyDocument` block, replace the delegate (current lines 99-101):

```rust
    // DELETE:
    // fn supports_comments(&self) -> bool {
    //     delegate!(self, d => d.supports_comments())
    // }
```

with:

```rust
    fn had_comments_at_open(&self) -> bool {
        delegate!(self, d => d.had_comments_at_open())
    }
```

In the test module, replace `json_from_str_enables_comments_from_content_only` (current lines 200-207):

```rust
    #[test]
    fn json_from_str_content_only_sets_had_comments_at_open() {
        // `had_comments_at_open` keys off content only — no other trigger exists.
        let plain = JsonDocument::from_str("{}\n").unwrap();
        assert!(!plain.had_comments_at_open());
        let commented = JsonDocument::from_str("// hi\n{}\n").unwrap();
        assert!(commented.had_comments_at_open());
    }
```

- [x] **Step 5: Update `TomlDocument` and `YamlDocument`**

In `crates/confy-core/src/model/cst_doc.rs`, delete the `supports_comments` impl (current lines 78-80):

```rust
    // DELETE:
    // fn supports_comments(&self) -> bool {
    //     true
    // }
```

Nothing replaces it — `TomlDocument` relies on the new trait default (`had_comments_at_open` → `false`).

In the test module, replace `toml_format_facets` (current lines 410-415), dropping the `supports_comments` assertion:

```rust
    #[test]
    fn toml_format_facets() {
        let doc = cst_from_str("a = 1\n");
        assert_eq!(doc.format(), DocFormat::Toml);
        assert_eq!(doc.comment_prefix(), "#");
    }
```

In `crates/confy-core/src/model/yaml/doc.rs`, delete the `supports_comments` impl (current lines 63-65):

```rust
    // DELETE:
    // fn supports_comments(&self) -> bool {
    //     true
    // }
```

In the test module, replace `roundtrip_and_facets` (current lines 271-280), dropping the `supports_comments` assertion:

```rust
    #[test]
    fn roundtrip_and_facets() {
        let src = "a: 1\nb: two\n";
        let doc = yaml_from_str(".yaml", src);
        assert_eq!(doc.serialize(), src);
        assert!(!doc.is_dirty());
        assert_eq!(doc.format(), DocFormat::Yaml);
        assert_eq!(doc.comment_prefix(), "#");
    }
```

- [x] **Step 6: Run confy-core tests**

Run: `cd crates/confy-core && cargo test`
Expected: PASS, zero failures. This also compiles the crate — any leftover `supports_comments`/`comments_enabled`/`enable_comments` reference anywhere else in `confy-core` (session layer, not yet touched by this task) will now fail to compile; that is expected and is fixed in Task 2.

- [x] **Step 7: Commit**

```bash
git add crates/confy-core/src/model/document.rs crates/confy-core/src/model/json/doc.rs crates/confy-core/src/model/any_doc.rs crates/confy-core/src/model/cst_doc.rs crates/confy-core/src/model/yaml/doc.rs
git commit -m "refactor(core): replace supports_comments() with content-only had_comments_at_open()"
```

---

### Task 2: `confy-core` session layer — remove the gate and the prompt

**Files:**
- Modify: `crates/confy-core/src/session/inline_edit.rs:359-372` (`split_value_comment` gate), `:906-943` (`add_comment_sibling` gate)
- Modify: `crates/confy-core/src/session/clipboard.rs:434-463` (`remark`)
- Modify: `crates/confy-core/src/session/session.rs:1965-1984` (prompt-accept handling)
- Modify: `crates/confy-core/src/session/state.rs:154-174` (`PromptKind`, `PendingComment`)
- Modify: `crates/confy-core/src/session/dispatch.rs:557-565,581-590` (`prompt_view`, `prompt_question`)
- Modify: `crates/confy-core/src/session/view.rs:150-156` (`PromptView`)
- Modify: `crates/confy-core/src/session/notice.rs:54,103,133` (`severity_of` table + its test)
- Modify: `crates/confy-core/src/session/mod.rs` (`PendingComment` re-export — found during execution, not anticipated by the plan's own investigation)
- Modify: `i18n/en.json`, `i18n/zh-TW.json` (delete `core.comment.unsupported`, `core.prompt.jsonc-upgrade`, `tui.prompt.jsonc-upgrade`, `tui.prompt.jsonc-upgrade.legend`)
- Test: `crates/confy-core/tests/session_headless.rs` — added `remark_never_prompts_on_clean_json` / `add_comment_sibling_never_blocked_on_clean_json` (integration tests via `Session::dispatch`, not the unit-level API the plan originally sketched — see Execution Status)

**Interfaces:**
- Consumes: `ConfigDocument::had_comments_at_open()` from Task 1 (not used in this task — this task only removes `supports_comments()` callers).
- Produces: `Session::add_comment_sibling`, `Session::remark`, `Session::split_value_comment` (all pre-existing method names/signatures, behavior only) become unconditionally legal on `.json`, matching TOML/YAML.

- [x] **Step 1: Delete the `split_value_comment` gate**

In `crates/confy-core/src/session/inline_edit.rs`, replace lines 364-368:

```rust
        let split = self
            .doc
            .as_ref()
            .filter(|d| d.supports_comments())
            .map(|d| d.split_value_comment(&raw_value));
```

with:

```rust
        let split = self.doc.as_ref().map(|d| d.split_value_comment(&raw_value));
```

- [x] **Step 2: Delete the `add_comment_sibling` gate**

In the same file, replace lines 906-914:

```rust
    fn add_comment_sibling(&mut self, target: Target) {
        let doc = match self.doc.as_mut() {
            Some(d) => d,
            None => return,
        };
        if !doc.supports_comments() {
            self.set_notice(Notice::core(self.lang, "core.comment.unsupported", &[]));
            return;
        }
```

with:

```rust
    fn add_comment_sibling(&mut self, target: Target) {
        let doc = match self.doc.as_mut() {
            Some(d) => d,
            None => return,
        };
```

(The rest of the function, lines 915-943, is untouched — it already starts with the "leading blank line" comment and continues unchanged.)

- [x] **Step 3: Delete the `remark` gate**

In `crates/confy-core/src/session/clipboard.rs`, replace lines 434-463:

```rust
    pub fn remark(&mut self) {
        if self.guard_clipboard_locked() {
            return;
        }
        if self.cursor_is_read_only() {
            self.set_notice(Notice::core(self.lang, "core.readonly", &[]));
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
```

with:

```rust
    pub fn remark(&mut self) {
        if self.guard_clipboard_locked() {
            return;
        }
        if self.cursor_is_read_only() {
            self.set_notice(Notice::core(self.lang, "core.readonly", &[]));
            return;
        }
        let path = match self.cursor_row() {
            Some(r) => r.path,
            None => return,
        };
        self.do_remark(path);
    }
```

- [x] **Step 4: Delete the `JsoncUpgrade` prompt-accept branch**

In `crates/confy-core/src/session/session.rs`, delete the whole match arm (current lines 1965-1984):

```rust
            // DELETE:
            // Mode::Prompt(PromptKind::JsoncUpgrade { .. }) => {
            //     match c {
            //         'y' | 'Y' => {
            //             if let Mode::Prompt(PromptKind::JsoncUpgrade { pending }) =
            //                 std::mem::replace(&mut self.mode, Mode::Normal)
            //             {
            //                 if let Some(d) = self.doc.as_mut() {
            //                     d.enable_comments();
            //                 }
            //                 match pending {
            //                     PendingComment::Remark { path } => self.do_remark(path),
            //                 }
            //             }
            //         }
            //         _ => {
            //             self.mode = self.resting_mode();
            //         }
            //     }
            //     false
            // }
```

The surrounding arms (`ArrayUpgrade` above, `ConfirmQuit` below) are untouched.

- [x] **Step 5: Delete `PromptKind::JsoncUpgrade` and `PendingComment`**

In `crates/confy-core/src/session/state.rs`, replace lines 154-174:

```rust
pub enum PromptKind {
    Collision {
        key: String,
    },
    ConfirmQuit,
    TypeChange {
        from: String,
        to: String,
    },
    ArrayUpgrade {
        target: Target,
        on_collision: crate::model::document::OnCollision,
    },
    JsoncUpgrade {
        pending: PendingComment,
    },
}

pub enum PendingComment {
    Remark { path: Path },
}
```

with:

```rust
pub enum PromptKind {
    Collision {
        key: String,
    },
    ConfirmQuit,
    TypeChange {
        from: String,
        to: String,
    },
    ArrayUpgrade {
        target: Target,
        on_collision: crate::model::document::OnCollision,
    },
}
```

(`PendingComment` is deleted entirely — no replacement, it had exactly one variant and no remaining caller.)

- [x] **Step 6: Delete the `PromptKind::JsoncUpgrade` projection arms in `dispatch.rs`**

In `crates/confy-core/src/session/dispatch.rs`, in `prompt_view` (current lines 557-565), delete the arm:

```rust
        // DELETE:
        // PromptKind::JsoncUpgrade { .. } => PromptView::JsoncUpgrade,
```

leaving:

```rust
fn prompt_view(pk: &PromptKind) -> PromptView {
    match pk {
        PromptKind::ConfirmQuit => PromptView::ConfirmQuit,
        PromptKind::Collision { .. } => PromptView::Collision,
        PromptKind::TypeChange { .. } => PromptView::TypeChange,
        PromptKind::ArrayUpgrade { .. } => PromptView::ArrayUpgrade,
    }
}
```

In `prompt_question` (current lines 581-590), delete the arm:

```rust
        // DELETE:
        // PromptKind::JsoncUpgrade { .. } => tr_args(lang, "core.prompt.jsonc-upgrade", &[]),
```

leaving:

```rust
pub fn prompt_question(lang: Lang, pk: &PromptKind) -> String {
    match pk {
        PromptKind::Collision { key } => tr_args(lang, "core.prompt.collision", &[key]),
        PromptKind::ConfirmQuit => tr_args(lang, "core.prompt.confirm-quit", &[]),
        PromptKind::TypeChange { from, to } => {
            tr_args(lang, "core.prompt.type-change", &[from, to])
        }
        PromptKind::ArrayUpgrade { .. } => tr_args(lang, "core.prompt.array-upgrade", &[]),
    }
}
```

- [x] **Step 7: Delete `PromptView::JsoncUpgrade`**

In `crates/confy-core/src/session/view.rs`, replace lines 150-156:

```rust
pub enum PromptView {
    ConfirmQuit,
    Collision,
    TypeChange,
    ArrayUpgrade,
    JsoncUpgrade,
}
```

with:

```rust
pub enum PromptView {
    ConfirmQuit,
    Collision,
    TypeChange,
    ArrayUpgrade,
}
```

- [x] **Step 8: Update `severity_of` and its test**

In `crates/confy-core/src/session/notice.rs`, in the `Warn` arm (current line 54), delete `| "core.comment.unsupported"`:

```rust
        "core.readonly" | "core.clipboard.action-locked"
        | "core.trailing.inline-unsupported" | "core.reveal.hidden-by-filter" | "core.move.self"
        | "core.insert.collision" | "core.rename.empty-key" | "core.value.invalid"
        | "core.comment.invalid" | "core.fragment.invalid" | "core.remark.invalid"
        | "core.convert.root-only" | "core.kind-switch.unsupported" | "core.schema.violation"
        | "web.host.fxios-save-hint" | "tui.host.readonly-comment"
        | "web.host.schema.load-error" | "tui.host.schema-load-error"
        | "web.host.json-comments-detected" | "tui.host.json-comments-detected" => Severity::Warn,
```

In the test module (current lines 84-137), delete the `("core.comment.unsupported", Severity::Warn),` entry (line 103) and fix the count assertion (line 133): the real catalog carried 42 cases (11 Error + 14 Warn + 7 Success + 9 Info + 1 controller-approved), so after removing one Warn case it becomes 41:

```rust
        assert_eq!(cases.len(), 41, "41 keys: §2.2's 40 (11 Error + 13 Warn + 7 Success + 9 Info) + controller-approved core.schema.violation (pass-through wrapper for the dynamic schema-violation advisory)");
```

- [x] **Step 9: Delete unused i18n keys from both catalogs**

In `i18n/en.json`, delete these four key-value lines:
- `"core.comment.unsupported": "comments not supported here",`
- `"core.prompt.jsonc-upgrade": "Introduce a // comment? This makes the file JSONC.",`
- `"tui.prompt.jsonc-upgrade": " Introduce a // comment? This makes the file JSONC.  y/n",`
- `"tui.prompt.jsonc-upgrade.legend": "y/n",`

In `i18n/zh-TW.json`, delete the corresponding four keys (same key strings, Traditional Chinese values).

- [x] **Step 10: Grep-locate and update session-layer tests referencing removed symbols**

Run: `cd crates/confy-core && grep -rn "supports_comments\|comments_enabled\|enable_comments\|JsoncUpgrade\|PendingComment\|core.comment.unsupported" src/session/`

Found `src/session/mod.rs`'s `PendingComment` re-export (not anticipated by the plan's own investigation) — removed it from the `pub use state::{…}` list alongside the same fix already applied in `state.rs`.

- [x] **Step 11: Run confy-core tests**

Run: `cd crates/confy-core && cargo test`
Expected: PASS, zero failures, zero warnings about unused `PendingComment`/`JsoncUpgrade` (they no longer exist).

- [x] **Step 12: Write and run the acceptance-criteria regression tests**

Added to `crates/confy-core/tests/session_headless.rs` (not `inline_edit.rs`/`clipboard.rs` unit tests as originally sketched — see Execution Status): `remark_never_prompts_on_clean_json` and `add_comment_sibling_never_blocked_on_clean_json`, both exercised via `Session::dispatch(Intent::…)` against a pure `.json` doc with zero comments at load.

- [x] **Step 13: Commit**

```bash
git add crates/confy-core/src/session/inline_edit.rs crates/confy-core/src/session/clipboard.rs crates/confy-core/src/session/session.rs crates/confy-core/src/session/state.rs crates/confy-core/src/session/dispatch.rs crates/confy-core/src/session/view.rs crates/confy-core/src/session/notice.rs crates/confy-core/src/session/mod.rs crates/confy-core/tests/session_headless.rs i18n/en.json i18n/zh-TW.json
git commit -m "feat(core): remove JSON comment write-gate and JsoncUpgrade prompt"
```

---

### Task 3: `confy-ffi` — drop the `supports_comments` binding, add `had_comments_at_open`

**Files:**
- Modify: `crates/confy-ffi/src/lib.rs:100-106`

**Interfaces:**
- Consumes: `ConfigDocument::had_comments_at_open()` from Task 1.
- Produces: `#[wasm_bindgen]` method `had_comments_at_open(&self) -> bool` on the FFI session wrapper type.

- [x] **Step 1: Replace the binding**

```rust
    /// Whether the open document already contained a comment when it was
    /// loaded — drives the host's one-shot "file already had comments" toast.
    pub fn had_comments_at_open(&self) -> bool {
        self.session
            .doc
            .as_ref()
            .is_some_and(|d| d.had_comments_at_open())
    }
```

- [x] **Step 2: Build confy-ffi natively**

Run: `cd crates/confy-ffi && cargo check` — succeeds.

- [x] **Step 3: Commit**

```bash
git add crates/confy-ffi/src/lib.rs
git commit -m "feat(ffi): replace supports_comments binding with had_comments_at_open"
```

---

### Task 4: `confy-tui` — rewire the toast, drop `load_document`'s `.jsonc` handling, fix tests

**Files:**
- Modify: `crates/confy-tui/src/tui/mod.rs:49-58`
- Modify: `crates/confy-tui/src/lib.rs:16-39,53-75` (`load_document` + its two tests)
- Modify: `crates/confy-tui/src/tui/app.rs:1832-1874` (`pure_json_remark_prompts_then_upgrades` test), `:3435-3444` (`app_with_jsonc` helper)
- Modify: `crates/confy-tui/src/tui/ui.rs:704-710` (`draw_prompt_overlay`'s `legend_key` match)
- Modify: `crates/confy-tui/src/tui/state.rs` (`PendingComment` re-export — found during execution)

**Interfaces:**
- Consumes: `ConfigDocument::had_comments_at_open()` (Task 1), `AnyDocument` no longer having `enable_comments` (Task 1).
- Produces: nothing consumed by later tasks (confy-tui is a leaf host).

- [x] **Step 1: Rewire the load-time toast condition** — `supports_comments()` → `had_comments_at_open()` in `tui/mod.rs`.
- [x] **Step 2: Delete `load_document`'s `.jsonc`-extension `enable_comments` call**, update its doc-comment.
- [x] **Step 3: Rewrite `load_document`'s comment tests** — `load_document_jsonc_extension_still_loads_correctly`, `load_document_json_with_existing_comment_sets_had_comments_at_open`, `load_document_pure_json_has_no_comments_at_open`.
- [x] **Step 4: Fix the `app_with_jsonc` test helper** — drop the `enable_comments()` call.
- [x] **Step 5: Rewrite `pure_json_remark_prompts_then_upgrades`** → `pure_json_remark_applies_immediately_no_prompt`.
- [x] **Step 6: Delete the `legend_key` match arm** for `PromptKind::JsoncUpgrade` in `ui.rs`.
- [x] **Step 6b (found during execution): fix `tui/state.rs`'s `PendingComment` re-export**, same pattern as `session/mod.rs` in Task 2.
- [x] **Step 7: Run confy-tui tests** — `cd crates/confy-tui && cargo test` — PASS, 0 failures (201 lib + 8 + 2 + 6 across integration suites).
- [x] **Step 8: Commit**

```bash
git add crates/confy-tui/src/tui/mod.rs crates/confy-tui/src/lib.rs crates/confy-tui/src/tui/app.rs crates/confy-tui/src/tui/ui.rs crates/confy-tui/src/tui/state.rs
git commit -m "feat(tui): rewire load-time toast to had_comments_at_open, drop JsoncUpgrade prompt"
```

---

### Task 5: `web` — drop the TypeScript callers and prompt wiring

**Files:**
- Modify: `web/confy.ts:86-89` — `supportsComments()` → `hadCommentsAtOpen()`
- Modify: `web/ui.ts:298` — toast condition rewired, stale comment updated
- Modify: `web/types.ts:92-97` — `PromptView` union drops `"JsoncUpgrade"`
- Modify: `web/prompt.ts:29-32,40-41` — `PROMPT_BUTTONS`/`PROMPT_TITLES` drop `JsoncUpgrade`
- Modify: `i18n/en.json`, `i18n/zh-TW.json` — delete `web.prompt.title.jsoncUpgrade`, `web.prompt.q.jsoncUpgrade`, `web.prompt.btn.upgradeJsonc`

**Interfaces:**
- Consumes: `had_comments_at_open` wasm-bindgen method name from Task 3.
- Produces: nothing consumed by later tasks.

- [x] **Steps 1-5**: applied as specified.
- [x] **Step 6: Grep-verify** — clean (only gitignored `web/dist/`, `web/pkg/`, `web/ui.js`, `web/touch/app.js` build-output copies showed stale content, expected until rebuilt).
- [x] **Step 7: Typecheck** — `web/tsconfig.json` exists; `npm run typecheck` failed until `web/pkg` was regenerated (Task 6 Step 4's wasm build), then passed clean.
- [x] **Step 8: Commit**

```bash
git add web/confy.ts web/ui.ts web/types.ts web/prompt.ts i18n/en.json i18n/zh-TW.json
git commit -m "feat(web): rewire load-time toast to hadCommentsAtOpen, drop JsoncUpgrade prompt"
```

---

### Task 6: Whole-workspace verification

- [x] **Step 1: Grep the entire workspace** — clean outside gitignored generated `pkg`/`dist`/bundled `.js` paths and this plan's/spec's own historical prose.
- [x] **Step 2: Build every native crate** — `cargo build -p confy-core -p confy-tui -p confy-ffi` — succeeds.
- [x] **Step 3: Run the full Rust test suite** — `cargo test --workspace` — PASS, 0 failures across every crate/integration suite.
- [x] **Step 4: Build the wasm target** — `wasm-pack build --target web` (in `crates/confy-ffi`) — succeeds; regenerated `crates/confy-ffi/pkg`.
  - Additionally ran `web/build.mjs` (copies `pkg/` into `web/pkg`, rebuilds `ui.js`/`touch/app.js`/`dist/`) and `editors/vscode/build.mjs` (copies into `editors/vscode/media/`) so every generated-artifact consumer picked up the new binding — not originally an explicit plan step, but required for Step 5's `tsc --noEmit` and the VS Code extension's own typecheck to pass clean.
- [x] **Step 5: Manually verify the load-time toast end-to-end** on the real `confy-tui` binary — confirmed via `hub` process control: a pre-commented `.json` scratch file shows the `tui.host.json-comments-detected` toast on open; a clean `.json` scratch file shows no toast, and pressing `r` (remark) applies the comment immediately with no y/n prompt.
- [x] **Step 6: Final commit** — folded into a follow-up `docs:` commit (`CLAUDE.md`/`docs/reference/CONTEXT.md` JSONC-upgrade description updates + a `CHANGELOG.md` entry), since those two doc files had been edited in an earlier phase of this session (spec-writing) but never committed.

## Assumptions & contingencies

- **`confy-ffi/src/lib.rs`'s exact `impl` block/struct name**: confirmed by direct read before editing — a plain `impl` method, no macro/codegen involved.
- **`web/tsconfig.json` presence**: present; `npm run typecheck` was run and is part of the verified record above.
- **Task 2 Step 10's session-layer test grep**: surfaced exactly one unanticipated hit (`session/mod.rs`'s `PendingComment` re-export), handled per the plan's own contingency note. A second, analogous unanticipated hit (`confy-tui/src/tui/state.rs`'s `PendingComment` re-export) was found in Task 4 and fixed the same way.
