# CONTEXT

Glossary for the confy codebase — a single-file TUI editor for structured config files
(TOML first). Resolved terminology only; no implementation details.

confy is modeled on [wenv](https://github.com/superyngo/wenv) for UX, but its domain language
is deliberately **different**: wenv is line-based and flat (`Group / File / Entry`); confy is a
**tree** over a structured document. confy does **not** use the term "Entry".

## Language

### Tree vocabulary

**Node**:
Any single element in the config tree. The umbrella term for everything the user navigates and
operates on. (Where wenv would say "Entry" — confy never says Entry.)
_Avoid_: Entry, item.

**Root**:
The single top-of-tree **Node** whose key is the filename. Every other Node descends from it.
There is exactly one Root per open file.
_Avoid_: File header (as a separate concept), top node.

**Branch node**:
A **Node** that has children and can be expanded/collapsed: a table, array-of-tables, array, or
inline table.
_Avoid_: Container, object, parent (parent is a relationship, not a kind).

**Leaf node**:
A **Node** with no children: a scalar value or a comment.
_Avoid_: Value node (a comment is a leaf but not a value), terminal.

**Parent / Child / Sibling**:
Standard tree relationships between **Nodes**. Siblings share a Parent and a key namespace
(the same table), which is why key collisions are resolved per-Parent.

### Node kinds

**Scalar**:
A **Leaf node** holding a typed TOML value — string, integer, float, bool, or datetime.

**Comment**:
A **standalone** comment line (occupies its own line) surfaced as a first-class **Leaf node**.
Navigable, selectable, remarkable, and movable like any Node. A "disabled" setting is just a
Comment whose text happens to be valid TOML; toggling it (see _Remark_) re-parses it back into a
live Node.
_Avoid_: Disabled entry, ghost node — these were earlier names for the same idea; the canonical
concept is "a Comment that is valid TOML".

**Trailing comment**:
An end-of-line comment that shares a line with a value (`port = 8080  # http`). It is **not** a
Node — it is decoration belonging to that Node, travels with it on edit/remark/move, and is shown
in the Node's Detail view. Only standalone comments become **Comment** Nodes.
_Avoid_: Inline comment node (it is never a node), suffix comment.

### Operations & projection

**Projection**:
The act of (re)building the Node tree from the backing document after every change. The backing
document — not the Node tree — is the single source of truth.

**Remark**:
The toggle that turns a live Node into a **Comment** (and back). Canonical name for what the
`r` key does.
_Avoid_: Disable/enable, comment-out (use these only as verbs in prose, never as the concept
name).

## Flagged ambiguities

- **"Entry" is banned in confy.** It is wenv's term for a flat, line-based item and means
  something materially different there (no children, a contiguous line range). confy's
  equivalent is **Node**, which is recursive. Any use of "Entry" in confy docs/code/UI copy is a
  bug to be renamed to Node.

## Example dialogue

> **Dev:** When the cursor is on `[server]` and I press `v` to paste, where does it land?
> **Domain expert:** `[server]` is a Branch node. Paste inserts the clipboard Nodes as new
> Children of it... no — as **Siblings** after the cursor, in the cursor's Parent. So the new
> Nodes share `[server]`'s Parent and key namespace.
> **Dev:** And if one of those Nodes is a comment like `# port = 8080`?
> **Domain expert:** That's a Comment leaf. It pastes as-is. If the user later presses `r` on
> it, Remark re-parses the text — it's valid TOML, so it becomes a live `port = 8080` Scalar.
> A `# just a note` Comment, by contrast, can't be Remarked into a live Node — it isn't valid
> TOML.
