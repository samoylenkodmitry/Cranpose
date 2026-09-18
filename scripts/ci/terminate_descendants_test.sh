#!/usr/bin/env bash
set -euo pipefail

# A cancelled CI job signals the step, not the tree under it. The robot suite
# answers that signal by calling terminate_descendants on itself, so whether
# that function really reaches a grandchild decides whether a cancelled job
# frees its runner and its host lock, or holds both for another hour.

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
# shellcheck disable=SC1091
. "$REPO_ROOT/scripts/dev_build_common.sh"

failures=0
check() {
    local description="$1"
    shift
    if "$@"; then
        echo "ok: $description"
    else
        echo "FAIL: $description" >&2
        failures=$((failures + 1))
    fi
}

# A parent that spawns a child that spawns its own child, all long lived.
work_dir="$(mktemp -d)"
trap 'rm -r -- "$work_dir" 2>/dev/null || true' EXIT

cat > "$work_dir/tree.sh" <<'TREE'
sleep 300 &
echo "$!" >> "$2"
bash -c 'sleep 300 & echo "$!" >> "'"$2"'"; sleep 300' &
echo "$!" >> "$2"
sleep 300
TREE

pids_file="$work_dir/pids"
: > "$pids_file"
bash "$work_dir/tree.sh" _ "$pids_file" &
root=$!

for _ in $(seq 1 100); do
    [ "$(grep -c '' "$pids_file")" -ge 3 ] && break
    sleep 0.1
done

mapfile -t descendants < "$pids_file" 2>/dev/null || descendants=($(cat "$pids_file"))
check "the fixture built a tree three deep (${#descendants[@]} descendants)" \
    [ "${#descendants[@]}" -ge 3 ]

alive=0
for pid in "${descendants[@]}"; do
    kill -0 "$pid" 2>/dev/null && alive=$((alive + 1))
done
check "every descendant is alive before the call ($alive)" \
    [ "$alive" -eq "${#descendants[@]}" ]

terminate_descendants "$root"

for _ in $(seq 1 100); do
    remaining=0
    for pid in "${descendants[@]}"; do
        kill -0 "$pid" 2>/dev/null && remaining=$((remaining + 1))
    done
    [ "$remaining" -eq 0 ] && break
    sleep 0.1
done

check "no descendant survives the call ($remaining left)" [ "$remaining" -eq 0 ]

kill -TERM "$root" 2>/dev/null || true
wait "$root" 2>/dev/null || true

if [ "$failures" -ne 0 ]; then
    echo "$failures descendant-termination check(s) failed" >&2
    exit 1
fi
echo "terminate_descendants: all checks passed"
