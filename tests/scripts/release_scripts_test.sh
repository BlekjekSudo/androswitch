#!/usr/bin/env bash
# Self-tests for the M12 release helpers (scripts/release-notes.sh and
# scripts/prune-merged-branches.sh). Hermetic: throwaway git repos and a
# stubbed `gh` on PATH, so no network and no token are needed.
#
#   bash tests/scripts/release_scripts_test.sh
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
NOTES="$ROOT/scripts/release-notes.sh"
PRUNE="$ROOT/scripts/prune-merged-branches.sh"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

fails=0
ok() { echo "ok   - $1"; }
ko() { echo "FAIL - $1" >&2; fails=$((fails + 1)); }
check() { if eval "$2"; then ok "$1"; else ko "$1"; fi; }

export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@example.invalid
export GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@example.invalid

# ── release-notes.sh ────────────────────────────────────────────────────────
repo="$WORK/notes"
git init -q "$repo"
(
  cd "$repo"
  git commit -q --allow-empty -m "chore: root"
  git tag v0.1.0
  # 250 commits since the previous tag: more than the 200-line cap, which is
  # exactly where `git log | head -n 200` died of SIGPIPE under pipefail.
  for i in $(seq 1 250); do git commit -q --allow-empty -m "feat: change $i"; done
  git tag v0.2.0
)

set +e
out="$(cd "$repo" && bash "$NOTES" v0.2.0)"
rc=$?
set -e
check "release-notes: >200 commits since last tag exits 0 (no SIGPIPE)" '[ "$rc" -eq 0 ]'
check "release-notes: commit list capped at 200 entries" \
  '[ "$(printf "%s\n" "$out" | grep -c "^- feat: change")" -eq 200 ]'
check "release-notes: range starts at the previous tag" \
  'printf "%s\n" "$out" | grep -q "Changes since v0.1.0"'

(
  cd "$repo"
  printf '# Changelog\n\n## [0.3.0]\n\n- curated entry\n\n## [0.2.0]\n\n- older\n' > CHANGELOG.md
  git add CHANGELOG.md
  git commit -q -m "docs: changelog"
  git tag v0.3.0
)
out="$(cd "$repo" && bash "$NOTES" v0.3.0)"
check "release-notes: curated CHANGELOG section wins" \
  'printf "%s\n" "$out" | grep -q "curated entry"'
check "release-notes: stops at the next heading" \
  '! printf "%s\n" "$out" | grep -q "older"'

set +e
(cd "$repo" && bash "$NOTES" >/dev/null 2>&1)
rc=$?
set -e
check "release-notes: missing tag argument is an error" '[ "$rc" -ne 0 ]'

# ── prune-merged-branches.sh (stubbed gh) ───────────────────────────────────
bin="$WORK/bin"
mkdir -p "$bin"
log="$WORK/gh.log"
: > "$log"
cat > "$bin/gh" <<'GH'
#!/usr/bin/env bash
# Minimal gh stub: fixed repo, 3 merged PRs, 4 live branches.
echo "$*" >> "$GH_STUB_LOG"
case "$1 $2" in
  "repo view")
    if [[ "$*" == *defaultBranchRef* ]]; then echo main; else echo o/r; fi ;;
  "pr list")
    cat <<'JSON'
[
 {"number": 10, "headRefName": "feat/merged-untouched", "headRefOid": "aaa", "mergedAt": "2026-10-25T00:00:00Z"},
 {"number": 11, "headRefName": "feat/moved-after-merge", "headRefOid": "bbb", "mergedAt": "2026-10-25T00:00:00Z"},
 {"number": 12, "headRefName": "main", "headRefOid": "mmm", "mergedAt": "2026-10-25T00:00:00Z"}
]
JSON
    ;;
  "api --paginate")
    printf 'main\tmmm\nfeat/merged-untouched\taaa\nfeat/moved-after-merge\tccc\nfeat/no-pr\tddd\n' ;;
  "api -X")
    ;;
  *) echo "unexpected gh call: $*" >&2; exit 2 ;;
esac
GH
chmod +x "$bin/gh"
export GH_STUB_LOG="$log"

out="$(PATH="$bin:$PATH" bash "$PRUNE" 2>&1)"
check "prune: dry run lists the merged, untouched branch" \
  'printf "%s\n" "$out" | grep -q "feat/merged-untouched"'
check "prune: branch that moved after its merge is skipped" \
  'printf "%s\n" "$out" | grep -q "skipped 1 branch" && ! printf "%s\n" "$out" | grep -q "feat/moved-after-merge.*merged PR"'
check "prune: default branch and branches without a PR are never listed" \
  '! printf "%s\n" "$out" | grep -qE "^  (main|feat/no-pr) "'
check "prune: dry run deletes nothing" '! grep -q "DELETE" "$log"'

: > "$log"
PATH="$bin:$PATH" bash "$PRUNE" --apply > /dev/null 2>&1
check "prune: --apply deletes exactly the one candidate" \
  '[ "$(grep -c "DELETE" "$log")" -eq 1 ] && grep -q "DELETE repos/o/r/git/refs/heads/feat/merged-untouched" "$log"'

if [ "$fails" -ne 0 ]; then
  echo "$fails check(s) failed" >&2
  exit 1
fi
echo "all release-script checks passed"
