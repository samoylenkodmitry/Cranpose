#!/usr/bin/env bash
set -euo pipefail

# A parallel robot worker is a fresh shell. It sees exactly two things: the
# functions run_robot_test.sh exports, and whatever sourcing
# scripts/dev_build_common.sh defines. A name `run_test` reaches that is in
# neither is not a missing command in one example -- it is every example in
# the parallel phase failing at once, and it only shows on a host that takes
# that path. `run_with_portable_timeout` and `host_load_1m` both went missing
# this way. This walks the call graph instead of trusting a list.

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
RUNNER="$REPO_ROOT/run_robot_test.sh"
COMMON="$REPO_ROOT/scripts/dev_build_common.sh"

defined_in() {
    sed -n 's/^\([a-z_][a-z0-9_]*\)() {$/\1/p' "$1"
}

body_of() {
    awk -v fn="$2" '$0 == fn "() {" { inside = 1; next } inside && /^}$/ { exit } inside' "$1"
}

runner_fns=$(defined_in "$RUNNER")
common_fns=$(defined_in "$COMMON")
exported=$(sed -n 's/^export -f \([a-z_][a-z0-9_]*\)$/\1/p' "$RUNNER")

resolve_body() {
    local fn="$1" out
    out="$(body_of "$RUNNER" "$fn")"
    [ -n "$out" ] && { printf '%s' "$out"; return; }
    body_of "$COMMON" "$fn"
}

# Transitive closure of what run_test calls.
pending="run_test"
seen=""
missing=""
while [ -n "$pending" ]; do
    fn="${pending%% *}"
    rest="${pending#"$fn"}"
    pending="${rest# }"
    case " $seen " in *" $fn "*) continue ;; esac
    seen="$seen $fn"

    # A worker reaches a name only via an export or via the sourced library.
    if [ "$fn" != run_test ]; then
        if ! grep -qx "$fn" <<< "$exported" && ! grep -qx "$fn" <<< "$common_fns"; then
            missing="$missing $fn"
            continue
        fi
    fi

    body="$(resolve_body "$fn")"
    for word in $(grep -oE '\b[a-z_][a-z0-9_]*\b' <<< "$body" | sort -u); do
        if grep -qx "$word" <<< "$runner_fns" || grep -qx "$word" <<< "$common_fns"; then
            pending="$pending $word"
        fi
    done
done

if [ -n "$missing" ]; then
    echo "FAIL: a parallel robot worker cannot reach:$missing" >&2
    echo "Export it from run_robot_test.sh, or define it in scripts/dev_build_common.sh," >&2
    echo "which every worker sources." >&2
    exit 1
fi

count=$(wc -w <<< "$seen" | tr -d ' ')
echo "robot worker contract: all $count function(s) reachable from run_test resolve in a worker"
