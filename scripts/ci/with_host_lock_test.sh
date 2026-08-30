#!/usr/bin/env bash
set -euo pipefail

# Regression test for with_host_lock.sh and the host capacity lock it
# applies (scripts/dev_build_common.sh owns the lock itself).
#
# scripts/dev_build_common.sh owns the reader/writer lock that keeps builds
# off the machine while a measurement runs, and the turnstile that keeps a
# stream of builds from starving that measurement forever. Both were verified
# by hand against the machine's real lock files while real CI jobs held them,
# which is not a thing to repeat: this suite drives the same code through
# CRANPOSE_HOST_LOCK_FILE / CRANPOSE_HOST_LOCK_TURNSTILE_FILE against a
# private pair under a scratch directory, so nothing here can touch a running
# job's lock and nothing a running job does can perturb a result here.
#
# The timing cases assert generous bounds, not tight ones, because this is a
# test of who gets the lock and not of how fast the machine is. Each bound
# sits between two outcomes that are far apart by construction: a fair writer
# waits for the readers already in flight (measured at 1-2s), a starved one
# waits out the entire 15s stream, and the bound is 6s.

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
readonly REPO_ROOT
readonly COMMON_SH="$REPO_ROOT/scripts/dev_build_common.sh"
readonly WITH_HOST_LOCK="$REPO_ROOT/scripts/ci/with_host_lock.sh"

# The defaults this suite must never write to, spelled out here on purpose:
# if someone changes them in dev_build_common.sh, the first case fails and
# says so rather than silently testing whatever the new value is.
readonly DEFAULT_LOCK_FILE="/tmp/cranpose-host-capacity.lock"
readonly DEFAULT_TURNSTILE_FILE="/tmp/cranpose-host-capacity.turnstile.lock"

# sccache has nothing to do with locking, and starting its daemon here would
# add seconds of noise to every case. A short lock wait keeps a regression
# failing in seconds instead of parking a CI job for the 45-minute default.
export CRANPOSE_USE_SCCACHE=0
export CRANPOSE_HOST_LOCK_MAX_WAIT_SECS=30
unset RUSTC_WRAPPER

# mktemp here rather than dev_build_common.sh's create_local_temp_dir: that
# file is what this suite tests, and sourcing it would fix its two lock
# constants -- they are readonly -- at the defaults inside the harness itself.
# Every case reaches the code under test through a child process instead, the
# same way CI does.
tmp_root="${TMPDIR:-/tmp}"
SCRATCH_DIR="$(mktemp -d "${tmp_root%/}/cranpose-host-lock-test.XXXXXX")"
readonly SCRATCH_DIR
export CRANPOSE_HOST_LOCK_FILE="$SCRATCH_DIR/host-capacity.lock"
export CRANPOSE_HOST_LOCK_TURNSTILE_FILE="$SCRATCH_DIR/host-capacity.turnstile.lock"

failures=0
cases_run=0
cases_skipped=0

cleanup() {
    # Kill anything still holding a scratch lock before the directory goes,
    # so a failed case cannot leave a background reader running.
    local pid
    for pid in $(jobs -pr); do
        kill "$pid" 2>/dev/null || true
    done
    wait 2>/dev/null || true
    if [ -d "$SCRATCH_DIR" ]; then
        rm -r -- "$SCRATCH_DIR" || true
    fi
}
trap cleanup EXIT

pass() {
    cases_run=$((cases_run + 1))
    echo "ok   - $1"
}

fail() {
    cases_run=$((cases_run + 1))
    failures=$((failures + 1))
    echo "FAIL - $1" >&2
    shift
    local line
    for line in "$@"; do
        echo "       $line" >&2
    done
}

skip() {
    cases_skipped=$((cases_skipped + 1))
    echo "skip - $1: $2"
}

expect_equal() {
    local name="$1" expected="$2" actual="$3"
    if [ "$expected" = "$actual" ]; then
        pass "$name"
    else
        fail "$name" "expected: $expected" "actual:   $actual"
    fi
}

# Sources dev_build_common.sh in a clean child and prints the two lock paths
# it resolved, one per line. Any leading arguments run as a wrapper around
# that child, which is how the defaults are read back with the overrides off.
resolved_lock_paths() {
    "$@" bash -c '. "$1"; printf "%s\n%s\n" "$HOST_CAPACITY_LOCK_FILE" "$HOST_CAPACITY_TURNSTILE_FILE"' \
        _ "$COMMON_SH"
}

# --- the paths themselves --------------------------------------------------

case_default_paths() {
    local resolved
    resolved="$(resolved_lock_paths env -u CRANPOSE_HOST_LOCK_FILE -u CRANPOSE_HOST_LOCK_TURNSTILE_FILE)"
    expect_equal "unset overrides resolve to the host-wide defaults" \
        "$DEFAULT_LOCK_FILE
$DEFAULT_TURNSTILE_FILE" "$resolved"
}

case_overridden_paths() {
    local resolved
    resolved="$(resolved_lock_paths)"
    expect_equal "the overrides replace both paths" \
        "$CRANPOSE_HOST_LOCK_FILE
$CRANPOSE_HOST_LOCK_TURNSTILE_FILE" "$resolved"
}

# --- the lock, through the overrides ---------------------------------------
#
# Everything below needs flock(1). macOS has none, which is why
# host_capacity_lock_available exists at all: there the wrapper runs its
# command unlocked, and there is no lock behaviour left to assert.

# Runs $@ under the wrapper in $mode, with its narration in the case log.
with_lock() {
    local mode="$1"
    shift
    "$WITH_HOST_LOCK" "$mode" "$@" >>"$SCRATCH_DIR/wrapper.log" 2>&1
}

# Inode, mode, size and mtime, or the fact that there is no such file. Enough
# to catch a run creating, truncating or replacing a path it was told not to
# touch, on a host where that path exists already and on one where it does not.
file_fingerprint() {
    if [ -e "$1" ]; then
        ls -ldi -- "$1"
    else
        echo "absent"
    fi
}

case_lock_lands_on_the_overridden_file() {
    local default_before default_after
    default_before="$(file_fingerprint "$DEFAULT_LOCK_FILE")"

    # `flock -n -x` from outside the wrapper is the only honest way to ask
    # whether the wrapper really holds the lock: the file existing proves
    # nothing, and the wrapper closes its own fds in the child.
    if with_lock --exclusive \
        bash -c '! flock -n -x "$CRANPOSE_HOST_LOCK_FILE" true'; then
        pass "the exclusive side holds the overridden lock file"
    else
        fail "the exclusive side holds the overridden lock file" \
            "$CRANPOSE_HOST_LOCK_FILE was lockable from outside the wrapper," \
            "or the wrapper never took it at all -- see the wrapper output below"
    fi

    default_after="$(file_fingerprint "$DEFAULT_LOCK_FILE")"
    expect_equal "an overridden run leaves the host-wide lock file alone" \
        "$default_before" "$default_after"
}

case_exclusive_waits_for_a_shared_holder() {
    local hold_secs=3 elapsed

    with_lock --shared sleep "$hold_secs" &
    local reader_pid=$!
    # Long enough for that reader to be holding the lock, not merely spawned.
    sleep 1

    local started=$SECONDS
    with_lock --exclusive true
    elapsed=$((SECONDS - started))
    wait "$reader_pid"

    # The reader holds for 3s and had 1s of it before the timer started, so a
    # working lock costs the writer ~2s. A broken one costs it none.
    if [ "$elapsed" -ge 1 ]; then
        pass "the exclusive side waits out a shared holder (waited ${elapsed}s)"
    else
        fail "the exclusive side waits out a shared holder" \
            "exclusive acquire returned in ${elapsed}s while a shared holder was still running"
    fi
}

# The regression this whole override exists for. flock(2) only ever checks
# locks currently held, never ones merely queued, so without the turnstile a
# continuous stream of shared acquirers is granted ahead of an already-waiting
# exclusive one for as long as the stream lasts -- fifteen seconds of stream
# kept a writer out for the full fifteen when this was measured. With the
# turnstile the writer's wait is bounded by the readers already in flight.
case_writer_is_not_starved_by_a_reader_stream() {
    local stream_secs=15 reader_hold=2 reader_gap=1 max_wait_secs=6 elapsed

    (
        stream_end=$((SECONDS + stream_secs))
        while [ "$SECONDS" -lt "$stream_end" ]; do
            with_lock --shared sleep "$reader_hold" &
            sleep "$reader_gap"
        done
        wait
    ) &
    local stream_pid=$!

    # Join mid-stream, so the writer is queueing against readers that are
    # already holding the lock and against new ones still arriving.
    sleep 3

    local started=$SECONDS
    local acquired=1
    with_lock --exclusive true || acquired=0
    elapsed=$((SECONDS - started))

    wait "$stream_pid" || true

    if [ "$acquired" = 0 ]; then
        fail "a reader stream does not starve the exclusive side" \
            "the exclusive acquire timed out entirely after ${elapsed}s"
    elif [ "$elapsed" -le "$max_wait_secs" ]; then
        pass "a reader stream does not starve the exclusive side (waited ${elapsed}s)"
    else
        fail "a reader stream does not starve the exclusive side" \
            "waited ${elapsed}s, over the ${max_wait_secs}s bound" \
            "a starved writer waits out the whole ${stream_secs}s stream; a fair one waits for the readers in flight"
    fi
}

# --- run -------------------------------------------------------------------

echo "host lock suite: locks under $SCRATCH_DIR"

case_default_paths
case_overridden_paths

if command -v flock >/dev/null 2>&1; then
    case_lock_lands_on_the_overridden_file
    case_exclusive_waits_for_a_shared_holder
    case_writer_is_not_starved_by_a_reader_stream
else
    skip "the lock cases" "no flock(1) on this host, so the wrapper runs unlocked here"
fi

echo "host lock suite: $cases_run run, $failures failed, $cases_skipped skipped"

if [ "$failures" -ne 0 ]; then
    echo "--- wrapper output ---" >&2
    cat "$SCRATCH_DIR/wrapper.log" >&2 2>/dev/null || true
    exit 1
fi
