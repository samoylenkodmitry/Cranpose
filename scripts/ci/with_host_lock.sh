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

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
. "$SCRIPT_DIR/../dev_build_common.sh"

readonly lock_file="/tmp/cranpose-host-capacity.lock"
# Long enough for a neighbour's Android or wasm build to finish, short enough
# to stay inside the robot job's 90-minute cap. On expiry the command runs
# anyway: a gate that will not start is worse than one that starts late, and
# the line printed here is what makes the resulting numbers questionable
# rather than mysterious.
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
if ! command -v flock >/dev/null 2>&1; then
  echo "with_host_lock: no flock on this host; running without the $mode lock"
  exec "$@"
fi

# Before the lock fd below exists, not after: this wrapper's whole job is to
# run a build (`--shared`) or a measurement (`--exclusive`) underneath it,
# and that command is frequently `cargo`. sccache's server is a long-lived
# daemon that cargo spawns lazily on the first wrapped rustc call, and a
# daemon spawned while fd 9 is open inherits a copy of it that it then holds
# for as long as it lives -- which is indefinite. Since flock's shared and
# exclusive modes are enforced across every open file description on the
# file, not just the one this script itself still holds, that inherited copy
# alone is enough to block every later exclusive acquire, even long after
# this script has exited. See run_robot_test.sh for the reproduction; the
# mechanism here is identical, just reached through the android/web/budgets
# `--shared` builds instead of the robot suite's own build step.
enable_local_sccache

exec 9>"$lock_file"

flock_mode="-s"
[[ "$mode" == "exclusive" ]] && flock_mode="-x"

if flock -n "$flock_mode" 9; then
  echo "with_host_lock: took the $mode lock on $lock_file immediately"
else
  echo "with_host_lock: $lock_file is busy -- waiting for the $mode lock..."
  wait_started_at=$(date +%s)
  if flock -w "$max_wait_seconds" "$flock_mode" 9; then
    echo "with_host_lock: took the $mode lock after $(( $(date +%s) - wait_started_at ))s"
  else
    echo "with_host_lock: gave up waiting for the $mode lock after ${max_wait_seconds}s" \
         "and started anyway. TREAT ANY TIMING RESULT BELOW AS UNMEASURED."
  fi
fi

# 8>&- 9>&-: never let the wrapped command inherit this lock's fd. If
# sccache's server is not already running (enable_local_sccache above failed
# or found nothing to start), the command spawning it here -- directly, or
# indirectly by execing further into another script -- must not hand it a
# copy of the lock to hold open forever. fd 8 is not opened by this script,
# but closing it too costs nothing and matches the same guard everywhere else
# this lock file is touched.
exec "$@" 8>&- 9>&-
