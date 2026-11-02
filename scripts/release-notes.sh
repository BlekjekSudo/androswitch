#!/usr/bin/env bash
# Release notes for a tag.
#
# Prefers the curated CHANGELOG section, falls back to the commit list since the
# previous tag. Kept in scripts/ (not inline in the workflow) so it can be run
# and tested locally: `scripts/release-notes.sh v0.9.0`.
set -euo pipefail

tag="${1:?usage: release-notes.sh <tag>}"
ver="${tag#v}"

if [ -f CHANGELOG.md ] && grep -qE "^## \[?${ver//./\\.}\]?" CHANGELOG.md; then
  # Print everything under the matching heading until the next `## ` heading.
  awk -v pat="^## \\[?${ver//./\\.}\\]?" '
    $0 ~ pat { inside = 1; next }
    inside && /^## / { exit }
    inside { print }
  ' CHANGELOG.md
  exit 0
fi

prev="$(git describe --tags --abbrev=0 "${tag}^" 2>/dev/null || true)"
if [ -n "$prev" ]; then range="${prev}..${tag}"; else range="${tag}"; fi

echo "### Changes since ${prev:-the first tag}"
echo
# `git log -n 200` (not `git log | head -n 200`): under `set -o pipefail`, head
# closing the pipe early makes git die of SIGPIPE (exit 141) and the whole
# script fails on repositories with more than 200 commits since the last tag.
git log -n 200 --no-merges --pretty='- %s (%h)' "$range"
echo
echo "Generated from git history because CHANGELOG.md has no \`${ver}\` section."
