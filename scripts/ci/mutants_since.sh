#!/usr/bin/env bash
# cargo-mutants on the Rust lines main changed since a point in time, for the
# nightly: each mutant is built and tested with its own package's tests, and
# the mutants no test caught are listed. A report: it never fails the job.
# After the given minutes it interrupts cargo-mutants, which stops and writes
# what it reached.
#
#   mutants_since.sh [since] [minutes]   # defaults: "24 hours ago", 25
set -uo pipefail

since_spec="${1:-24 hours ago}"
minutes="${2:-25}"
summary="${GITHUB_STEP_SUMMARY:-/dev/stdout}"
out="target/mutants-since"

since="$(git rev-list -1 --before="$since_spec" HEAD)"
if [ -z "$since" ]; then
  echo "mutants: no commit before $since_spec"
  exit 0
fi
mkdir -p "$out"
git diff "$since" HEAD -- '*.rs' ':(exclude)**/tests/**' ':(exclude)**/*_tests.rs' > "$out/changes.diff"
if [ ! -s "$out/changes.diff" ]; then
  echo "mutants: no Rust change outside tests since ${since:0:9}"
  exit 0
fi

rm -f "$out/stopped"
cargo mutants --in-diff "$out/changes.diff" --output "$out" &
mutants=$!
(sleep "$((minutes * 60))" && touch "$out/stopped" && kill -INT "$mutants" 2>/dev/null) &
cap=$!
wait "$mutants"
status=$?
kill "$cap" 2>/dev/null

{
  echo "## Mutants of the changes since ${since:0:9}"
  if [ -e "$out/stopped" ]; then
    echo "Stopped after ${minutes} minutes; the mutants run in random order, so each night samples others."
  elif [ "$status" -eq 0 ]; then
    echo "Every mutant was caught or did not build."
  else
    echo "cargo-mutants exited $status."
  fi
  if [ -s "$out/mutants.out/missed.txt" ]; then
    echo
    echo "Mutants no test caught:"
    echo '```'
    cat "$out/mutants.out/missed.txt"
    echo '```'
  fi
} >> "$summary"
exit 0
