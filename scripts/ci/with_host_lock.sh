#!/usr/bin/env bash
set -euo pipefail

# Reader/writer lock over "this machine's CPU", so that the jobs which MEASURE
# time never run beside the jobs which merely CONSUME it.
#
#   --shared     a build. Any number may hold this at once.
#   --exclusive  a measurement. One at a time, and no build while it runs.
#
# One Linux machine serves every `[self-hosted, Linux, cranpose-heavy]` job.
# Two runners are registered on it, so two heavy jobs land there at once, and
# a heavy job here means twelve rustc processes. The robot suite's per-frame
# assertions cannot survive that: `robot_text_handle_cycle_stability` failed
# on main at `drag work_avg_ms 0.73 -> 1.66`, then passed on the same host on
# that commit AND on the commit before it once the box was quiet. Measured
# during one such failure, the one-minute load average was 61 on twelve cores.
#
# Holding only the robot suites apart from each other is not enough, and
# letting the rest of the fleet schedule independently buys throughput at the
# cost of the answer: the neighbour it lets through is exactly what makes the
# measurement wrong, and a fast wrong answer is not throughput.
#
# Shared/exclusive rather than one exclusive lock for everything, because
# builds do not interfere with each other's correctness -- only with a
# measurement's. Builds still overlap builds; only a measurement empties the
# machine.
#
# flock is the right primitive because the kernel -- not this script -- owns
# releasing it: the lock lives on an open file descriptor, and closing that
# descriptor (script exits, errors out, or is killed by a cancelled job)
# releases it immediately. There is no PID file to go stale and no cleanup
# step that a crash can skip.
#
# Plain flock is not enough by itself, though: it only ever checks locks
# *currently held*, never ones merely queued, so a continuous stream of new
# --shared requests is granted ahead of an already-waiting --exclusive one
# forever. scripts/dev_build_common.sh's host_capacity_turnstile_* functions
# close that gap; see the comment there for the experiment that proved it and
# the fix's shape. This wrapper and run_robot_test.sh's own two-phase
# acquisition both call the same functions rather than each hand-rolling this
# logic -- a second copy is exactly the kind of drift that let the two
# diverge in the first place (this file did not take the lock at all for the
# exclusive side; run_robot_test.sh had already grown its own copy).

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
. "$SCRIPT_DIR/../dev_build_common.sh"

# Long enough for a neighbour's Android or wasm build to finish, short enough
# to stay inside the robot job's 90-minute cap.
readonly max_wait_seconds="${CRANPOSE_HOST_LOCK_MAX_WAIT_SECS:-2700}"

mode=""
case "${1:-}" in
  --shared) mode="shared"; shift ;;
  --exclusive) mode="exclusive"; shift ;;
  *)
    echo "usage: $0 --shared|--exclusive <command> [args...]" >&2
    exit 64
    ;;
esac

if [[ "$#" -eq 0 ]]; then
  echo "usage: $0 --shared|--exclusive <command> [args...]" >&2
  exit 64
fi

# macOS has no flock(1), and the machine this protects is the Linux one. A
# host that cannot lock runs the command rather than pretending it waited.
if ! host_capacity_lock_available; then
  echo "with_host_lock: no flock on this host; running without the $mode lock"
  exec "$@"
fi

# Before the lock fd below exists, not after: this wrapper's whole job is to
# run a build (`--shared`) or a measurement (`--exclusive`) underneath it,
# and that command is frequently `cargo`. sccache's server is a long-lived
# daemon that cargo spawns lazily on the first wrapped rustc call, and a
# daemon spawned while a lock fd is open inherits a copy of it that it then
# holds for as long as it lives -- which is indefinite. Since flock's shared
# and exclusive modes are enforced across every open file description on the
# file, not just the one this script itself still holds, that inherited copy
# alone is enough to block every later exclusive acquire, even long after
# this script has exited. See run_robot_test.sh for the reproduction; the
# mechanism here is identical, just reached through the android/web/budgets
# `--shared` builds instead of the robot suite's own build step.
enable_local_sccache

flock_mode="-s"
fail_on_timeout=0
if [[ "$mode" == "exclusive" ]]; then
  flock_mode="-x"
  fail_on_timeout=1
  host_capacity_turnstile_hold
else
  host_capacity_turnstile_pass
fi

exec 9>"$HOST_CAPACITY_LOCK_FILE"

if ! host_capacity_flock_wait 9 "$flock_mode" "$max_wait_seconds" \
    "the $mode lock on $HOST_CAPACITY_LOCK_FILE" "$fail_on_timeout"; then
  exit 1
fi

[[ "$mode" == "exclusive" ]] && host_capacity_turnstile_release

# Not `exec`: this process has to stay alive holding fd 9 for as long as the
# wrapped command actually runs, or the lock's real duration is however long
# bash takes to set up a redirect, not however long the build or measurement
# takes. `exec "$@" 9>&-` replaces this process with $@ AS PART OF applying
# this exec's own redirections, and those are applied -- fd 9 closed -- before
# $@'s image loads, releasing the flock immediately. Proved on samarch-1:
# `exec sleep 5 9>&-` released a held exclusive lock in 0.00s, four seconds
# before the sleep it was supposedly still protecting finished. `"$@" 9>&-`
# as a plain foreground command instead forks $@ as a child with fd 9 (and 7,
# 8) closed in that CHILD ONLY -- so nothing it spawns, sccache's daemon
# included, can inherit a copy either -- while this process keeps fd 9 open
# until the child actually exits, then exits with its status.
"$@" 7>&- 8>&- 9>&-
