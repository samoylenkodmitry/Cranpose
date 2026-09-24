#!/usr/bin/env bash
set -euo pipefail

# Remembers which job already passed on which source tree, so a job does not
# repeat work this runner host has already proven.
#
#   tree_memo.sh check  KEY   writes passed=true|false to $GITHUB_OUTPUT
#   tree_memo.sh record KEY   marks the checked-out tree as passed for KEY
#
# The tree hash covers every tracked file: sources, the workflow that runs
# the job, the justfile and these scripts. Two checkouts with the same tree
# hash ask a job the same question, and the commonest pair is a pull
# request's last run and the merge that lands it: GitHub tests the pull
# request as a merge with main, and when main has not moved since, the merge
# commit on main has that exact tree. On 2026-09-24, 8 of 14 merges landed a
# tree their pull request had already passed, and main ran every job again.
#
# The memo lives on the runner host, not in GitHub: a job that lands on a
# host that has not seen the tree simply runs. A marker older than
# CRANPOSE_CI_MEMO_MAX_AGE_MINUTES (default 720) does not count, so a
# toolchain or driver change on the host is re-proven within half a day.

mode="${1:?usage: tree_memo.sh check|record KEY}"
key="${2:?usage: tree_memo.sh check|record KEY}"
max_age_minutes="${CRANPOSE_CI_MEMO_MAX_AGE_MINUTES:-720}"
real_home="$(eval echo "~$(id -un)")"
memo_root="${CRANPOSE_CI_MEMO_DIR:-$real_home/.cache/cranpose/ci-passed}"
key_dir="$memo_root/$(printf '%s' "$key" | tr -c 'A-Za-z0-9._-' '_')"
tree="$(git rev-parse 'HEAD^{tree}')"
marker="$key_dir/$tree"

case "$mode" in
    check)
        passed=false
        if [[ -f "$marker" ]] && [[ -n "$(find "$marker" -mmin "-$max_age_minutes" 2>/dev/null)" ]]; then
            passed=true
            echo "tree $tree already passed $key on this host; skipping its work"
        fi
        if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
            echo "passed=$passed" >> "$GITHUB_OUTPUT"
        else
            echo "passed=$passed"
        fi
        ;;
    record)
        mkdir -p "$key_dir"
        touch "$marker"
        find "$key_dir" -type f -mtime +7 -delete 2>/dev/null || true
        echo "recorded that tree $tree passed $key"
        ;;
    *)
        echo "unknown mode \`$mode\`; use check or record" >&2
        exit 2
        ;;
esac
