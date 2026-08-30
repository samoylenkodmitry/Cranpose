#!/usr/bin/env bash
# Refuse to start a build that the disk cannot finish.
#
# Without this, exhaustion arrives as `No space left on device (os error 28)`
# from somewhere deep in a link step, or -- worse, and this is what happened on
# 2026-08-29 -- as an agent whose shell can no longer create a file to capture
# output, which looks like the tool being broken rather than the disk being
# full. A build that refuses to start with a number and an instruction costs
# minutes; one that dies at 95% costs the whole build and the diagnosis after
# it.
#
# Under pressure this first tries to fix the problem rather than only reporting
# it: worktree target dirs are caches (see target_gc.sh), so it evicts the
# least-recently-built ones and re-checks before giving up. Only a disk that is
# still short after that is a real failure.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# is_ci_env, so "local build" means the same thing here as everywhere else in
# this repo rather than being decided twice.
# shellcheck disable=SC1091
. "$SCRIPT_DIR/../dev_build_common.sh"

# Below this, a build is refused outright.
readonly HARD_FLOOR_GB="${CRANPOSE_DISK_MIN_GB:-25}"
# Below this, warn and sweep, but let the build proceed.
readonly SOFT_FLOOR_GB="${CRANPOSE_DISK_WARN_GB:-60}"
# What the automatic sweep aims for once it decides to run.
readonly SWEEP_TARGET_GB="${CRANPOSE_DISK_SWEEP_TARGET_GB:-120}"
# The automatic sweep is deliberately more cautious than a hand-run one: two
# hours, not fifteen minutes, so it cannot evict a target belonging to a session
# that merely paused to think.
readonly SWEEP_BUSY_MINUTES="${CRANPOSE_DISK_SWEEP_BUSY_MINUTES:-120}"

free_gb() {
    local gb
    gb="$( { df -k "${1:-$PWD}" 2>/dev/null || true; } | awk 'NR == 2 { printf "%d", $4 / 1048576 }')"
    [ -n "$gb" ] || gb=0
    printf '%s\n' "$gb"
}

sweep() {
    echo "disk guard: sweeping least-recently-built worktree target dirs..." >&2
    "$SCRIPT_DIR/target_gc.sh" --apply \
        --min-free-gb "$SWEEP_TARGET_GB" \
        --busy-minutes "$SWEEP_BUSY_MINUTES" >&2 || true
}

main() {
    local label="${1:-this build}"
    local free
    free="$(free_gb "$PWD")"

    if [ "$free" -ge "$SOFT_FLOOR_GB" ]; then
        return 0
    fi

    echo "disk guard: ${free}G free, under the ${SOFT_FLOOR_GB}G comfort floor." >&2

    # Advisory only in CI. The runners are shared -- samarch-1 carries nineteen
    # repositories' runners -- so a sweep there would delete build state this
    # job does not own the decision for, and a hard refusal would turn a runner
    # that has always built fine near its floor into a red build. Locally the
    # disk belongs to one person and both actions are wanted.
    if is_ci_env; then
        echo "disk guard: CI, so reporting only -- not sweeping, not refusing." >&2
        return 0
    fi

    if [ "${CRANPOSE_DISK_AUTOSWEEP:-1}" = "1" ]; then
        sweep
        free="$(free_gb "$PWD")"
        echo "disk guard: ${free}G free after sweep." >&2
    fi

    if [ "$free" -lt "$HARD_FLOOR_GB" ]; then
        cat >&2 <<EOF

disk guard: refusing to start $label with ${free}G free (floor: ${HARD_FLOOR_GB}G).

A cargo build of this workspace needs tens of gigabytes. Starting now would most
likely fail as "No space left on device (os error 28)" partway through, or take
the shell down with it.

Everything the automatic sweep could safely reclaim is already gone: what is
left is either being built right now or belongs to this worktree. To see the
full picture and decide yourself:

    just gc                      # report only, removes nothing
    just gc-apply                # reclaim to the default free-space target
    scripts/dev/target_gc.sh --min-free-gb 250 --busy-minutes 5

Only cargo target directories are ever removed, and only ones carrying cargo's
own CACHEDIR.TAG marker. Source and uncommitted work are never touched.
EOF
        return 1
    fi

    return 0
}

main "$@"
