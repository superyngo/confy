---
status: accepted
---

# Editor-outline symbol ranges for scattered-definition nodes anchor at the first member, never an envelope

## Context

The VS Code `DocumentSymbolProvider` design (`docs/superpowers/specs/2026-08-20-vscode-outline-provider-design.md`)
needs a single contiguous `text_range: Range<usize>` per `Node`, because
`vscode.DocumentSymbol.range` only accepts one contiguous range. But
`CONTEXT.md`'s existing **Member spans** concept establishes that a table's
definition in TOML is an *open set*: its own `[a]` section, every descendant
`[a.sub]`/`[[a.list]]` section wherever it sits in the file, and any flat
dotted member lines — these can scatter and interleave with unrelated
foreign sections. Some of these nodes (`Format::Dotted` synthetic tables
specifically) have no backing `rowan::SyntaxNode` at all — they are a
projection-time merge of multiple separate `SyntaxNode`s
(`dotted_member_entries()` returns a `Vec<SyntaxNode>`), so there is no
single span to read off the syntax tree the way every other node kind gets
one for free.

Two options existed: compute a min-max envelope over every scattered piece
(so the reported range visually covers everything, including unrelated
interleaved content), or anchor at a single representative member and accept
that the range is not a full envelope.

## Decision

Editor-outline `text_range`/`key_text_range` for a scattered-definition node
anchors at its **first member's own range**, never a min-max envelope:

- A `Format::Dotted` synthetic Table's `text_range` is its first member's
  `text_range()` — the same "first definition position" `CONTEXT.md` already
  documents as where a consolidating block-rewrite lands for that table.
- A normal Table whose descendant sub-sections are defined non-adjacently
  keeps its own `text_range` scoped to its own header + directly-owned
  entries; it does not widen to enclose a scattered descendant's range.
  VS Code does not enforce that a parent `DocumentSymbol.range` encloses its
  children's ranges (only a loose convention) — the outline tree still
  nests correctly, each range is simply honest about its own source text.

## Considered options

- **Min-max envelope over all scattered members** — rejected: for the
  interleaved case (`a.b = 1` / `x = 0` / `a.c = 2`), the envelope would
  claim the Dotted table's clickable/highlightable range covers the
  unrelated `x = 0` line in between, which is actively misleading rather
  than just imprecise.
- **Exclude scattered-definition container nodes from the outline
  entirely, flattening their children up a level** — rejected: this was
  considered specifically for `Format::Dotted` tables and would remove the
  grouping signal the outline exists to provide, in exchange for sidestepping
  a problem the "first member" anchor already solves without losing anything.

## Consequences

- No FFI/wire-format ambiguity: every `Node`'s `text_range` is defined
  precisely, including the previously-unaddressed `Format::Dotted` case.
- A future consumer (e.g. `web/breadcrumb.ts`, if it adopts these fields
  later per the design doc's non-goal) inherits the same anchoring rule for
  free — this decision is not VS Code-specific, it is a property of the
  `Node.text_range` field itself.
- Clicking a Dotted table's symbol in VS Code's Outline/breadcrumbs jumps to
  its first member's line, not a synthesized "whole table" region — this is
  the intended, documented behavior, not a known limitation to fix later.
