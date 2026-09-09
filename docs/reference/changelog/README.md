# Changelog archives

The root [`CHANGELOG.md`](../../../CHANGELOG.md) carries **`[Unreleased]` plus the current
version series only**. Completed series are moved here verbatim — same format, same ordering,
no edits.

| Archive | Covers |
|---|---|
| [`v0.x.md`](v0.x.md) | v0.2.0 (2026-06-06) … v0.32.0 (2026-09-01) |

**When to archive.** On the first release of a new major series (v2.0.0), move the whole
preceding series into `v1.x.md` here and add a row above. Never archive the series the next tag
belongs to: `.github/workflows/release.yml`'s `verify-versions` job greps the **root**
`CHANGELOG.md` for `## [vX.Y.Z]` and hard-fails the tagged build if it is missing.
