#!/usr/bin/env bash
set -euo pipefail

# Serialises robot-suite invocations against each other on the physical
# machine that runs them, without serialising anything else.
#
# `robot e2e (self-hosted GPU)` and `robot external captures (software
# present)` in heavy-selfhosted.yml both drive the same GPU and X server.
# Two Linux runners on one host can now pick either job up independently,
# so nothing about runner assignment stops them from landing on the same
# box at the same time -- which starves both of CPU and produces flaky
# robot failures that are host contention, not code (see PLAN.md's
# Infrastructure section). GitHub's `concurrency:` key cannot express "not
# while a job on this host is doing something else": it only supersedes
# older runs of the *same* workflow ref. A host-level file lock can.
#
# flock is the right primitive because the kernel -- not this script --
# owns releasing it: the lock lives on an open file descriptor, and closing
# that descriptor (script exits, errors out, or is killed by a cancelled
# job) releases it immediately. There is no PID file to go stale and no
# cleanup step that a crash can skip.
#
# Only the robot-suite steps should ever call this. Everything else in the
# fleet (architecture budgets, wasm build, Android/iOS builds) must keep
# scheduling independently of robot-suite activity.

readonly lock_file="/tmp/cranpose-robot-suite.lock"

if [[ "$#" -eq 0 ]]; then
  echo "usage: $0 <command> [args...]" >&2
  exit 64
fi

exec 9>"$lock_file"

if flock -n 9; then
  echo "with_robot_host_lock: acquired $lock_file immediately"
else
  echo "with_robot_host_lock: $lock_file is held by another robot job on this host -- waiting..."
  wait_started_at=$(date +%s)
  flock 9
  waited_seconds=$(( $(date +%s) - wait_started_at ))
  echo "with_robot_host_lock: acquired $lock_file after waiting ${waited_seconds}s"
fi

exec "$@"
