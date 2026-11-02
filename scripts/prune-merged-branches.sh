#!/usr/bin/env bash
# Delete remote branches whose pull request was MERGED.
#
# Deliberately conservative: a branch is only removed when GitHub reports a
# merged PR for it. Branches without a PR, closed-but-not-merged PRs, and the
# default branch are never touched. Dry-run is the default; pass --apply to
# delete. Requires `gh auth` with repo scope.
#
#   scripts/prune-merged-branches.sh          # list what would be deleted
#   scripts/prune-merged-branches.sh --apply  # delete them
set -euo pipefail

apply=0
[ "${1:-}" = "--apply" ] && apply=1

repo="${GH_REPO:-$(gh repo view --json nameWithOwner -q .nameWithOwner)}"
default_branch="$(gh repo view "$repo" --json defaultBranchRef -q .defaultBranchRef.name)"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# Every merged PR in the repo, newest first.
gh pr list --repo "$repo" --state merged --limit 1000 \
  --json number,headRefName,headRefOid,mergedAt > "$tmp/merged.json"

# name<TAB>sha of every live branch
gh api --paginate "repos/$repo/branches?per_page=100" \
  -q '.[] | "\(.name)\t\(.commit.sha)"' > "$tmp/live_branches.txt"

python3 - "$tmp" "$default_branch" > "$tmp/candidates.txt" <<'PY'
import json, sys, pathlib

tmp = pathlib.Path(sys.argv[1])
default_branch = sys.argv[2]

merged = json.loads((tmp / "merged.json").read_text())

live = {}
for line in (tmp / "live_branches.txt").read_text().splitlines():
    if not line.strip():
        continue
    name, _, sha = line.partition("\t")
    live[name] = sha.strip()

branches = {}
for pr in merged:
    name = pr.get("headRefName")
    if not name:
        continue
    branches.setdefault(name, []).append(pr)

skipped = []
for name in sorted(live.keys() & branches.keys()):
    if name == default_branch:
        continue
    prs = branches[name]
    # Only delete when the live tip is still the commit the merged PR ended on.
    # A branch that moved after the merge may hold unpushed-in-review work.
    if live[name] and any(pr.get("headRefOid") == live[name] for pr in prs):
        print(f"{name}\t{','.join('#' + str(pr['number']) for pr in sorted(prs, key=lambda p: p['number']))}")
    else:
        skipped.append(name)

if skipped:
    import sys as _sys
    print(f"# skipped {len(skipped)} branch(es) that moved after the merge: "
          + ", ".join(skipped[:10]), file=_sys.stderr)
PY

count="$(wc -l < "$tmp/candidates.txt" | tr -d ' ')"
if [ "$count" -eq 0 ]; then
  echo "Nothing to prune: every live branch either has no merged PR or is the default branch."
  exit 0
fi

echo "$count branch(es) with a merged PR:"
awk -F'\t' '{ printf "  %-55s (merged PR %s)\n", $1, $2 }' "$tmp/candidates.txt"

if [ "$apply" -ne 1 ]; then
  echo
  echo "Dry run. Re-run with --apply to delete these remote branches."
  exit 0
fi

while IFS=$'\t' read -r name prs; do
  echo "deleting $name (merged PR $prs)"
  gh api -X DELETE "repos/$repo/git/refs/heads/$name" >/dev/null
done < "$tmp/candidates.txt"

echo "done."
