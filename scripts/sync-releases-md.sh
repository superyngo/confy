#!/usr/bin/env bash
# Patches RELEASES.md's "Current version" column for one channel row and
# pushes the change to main. Invoked from release.yml / publish-msstore.yml /
# publish-vscode.yml right after each channel actually goes live, so the
# table stops drifting from reality (previously a manual, easy-to-forget
# step — see CHANGELOG history for "docs: update RELEASES.md versions").
#
# Usage: sync-releases-md.sh "<row anchor text>" "<version>"
#   <row anchor text>  substring unique to one "Platform / channel" table row,
#                       e.g. "TUI binaries" or "Windows Microsoft Store".
#   <version>           value to write into that row's "Current version"
#                       column, e.g. "v0.19.0".
#
# Assumes the working tree is already checked out on `main` and that
# `git push` can reach `origin` (GITHUB_TOKEN with contents:write).
set -euo pipefail

anchor="$1"
version="$2"
file="RELEASES.md"

if ! grep -qF "$anchor" "$file"; then
  echo "::error::no RELEASES.md row matches anchor '$anchor'" >&2
  exit 1
fi

# RELEASES.md's table rows have no literal "|" inside cell text, so a plain
# FS='|' split is safe. Table layout: | Platform | Method | Trigger |
# Current version | Status | -> fields (1-indexed, FS='|'): 1 "", 2 Platform,
# 3 Method, 4 Trigger, 5 Current version, 6 Status, 7 "".
awk -F'|' -v OFS='|' -v anchor="$anchor" -v ver="$version" '
  index($0, anchor) { $5 = " " ver " " }
  { print }
' "$file" > "$file.tmp"
mv "$file.tmp" "$file"

if git diff --quiet -- "$file"; then
  echo "RELEASES.md already at $version for '$anchor' -- nothing to commit"
  exit 0
fi

git config user.name "github-actions[bot]"
git config user.email "41898282+github-actions[bot]@users.noreply.github.com"
git add "$file"
git commit -m "docs: RELEASES.md -- ${anchor} -> ${version}"

# msstore and vscode publishing can be approved around the same time, so both
# workflows may race to push this file -- retry on a rejected non-fast-forward
# push instead of failing the whole publish job over a docs conflict.
for attempt in 1 2 3 4 5; do
  if git push origin HEAD:main; then
    exit 0
  fi
  echo "push rejected (attempt $attempt/5), rebasing onto origin/main..." >&2
  git fetch origin main
  git rebase origin/main
done

echo "::error::failed to push RELEASES.md update after 5 attempts" >&2
exit 1
