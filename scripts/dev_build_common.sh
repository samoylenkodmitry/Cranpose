#!/bin/bash

find_local_tool() {
    local tool_name="$1"
    if command -v "$tool_name" >/dev/null 2>&1; then
        command -v "$tool_name"
        return 0
    fi
    if [ -x "$HOME/.cargo/bin/$tool_name" ]; then
        printf '%s\n' "$HOME/.cargo/bin/$tool_name"
        return 0
    fi
    return 1
}

is_ci_env() {
    [ -n "${CI:-}" ] || [ -n "${GITHUB_ACTIONS:-}" ]
}

enable_local_sccache() {
    local sccache_bin="${RUSTC_WRAPPER:-}"

    if [ -z "$sccache_bin" ]; then
        if is_ci_env || [ "${CRANPOSE_USE_SCCACHE:-1}" = "0" ]; then
            return 0
        fi
        if ! sccache_bin="$(find_local_tool sccache)"; then
            return 0
        fi
        export RUSTC_WRAPPER="$sccache_bin"
    fi

    # Start the daemon now, on purpose, before any caller has a chance to
    # open a lock fd: sccache spawns its server lazily on the first wrapped
    # rustc call, and that server is a long-lived daemon that inherits
    # whatever file descriptors its parent (cargo, and above it this script)
    # happens to have open at that moment. A flock fd leaked into it that way
    # stays held for the daemon's lifetime, which can be forever -- see
    # run_robot_test.sh's host-capacity lock for why that matters. Starting
    # the server here, before a caller's lock section begins, means the later
    # build never needs to spawn it. --start-server is a no-op against an
    # already-running server, so calling this more than once is harmless.
    # Only ever touch a binary actually named sccache: RUSTC_WRAPPER can
    # point at anything, and most wrappers do not understand this flag.
    case "$(basename -- "$sccache_bin")" in
        sccache|sccache.exe) ;;
        *) return 0 ;;
    esac
    command -v "$sccache_bin" >/dev/null 2>&1 || return 0
    "$sccache_bin" --start-server >/dev/null 2>&1 || true
}

local_temp_root() {
    if [ -n "${CRANPOSE_TMPDIR:-}" ]; then
        printf '%s\n' "$CRANPOSE_TMPDIR"
        return 0
    fi

    if [ -n "${XDG_CACHE_HOME:-}" ]; then
        printf '%s\n' "$XDG_CACHE_HOME/cranpose/tmp"
        return 0
    fi

    printf '%s\n' "$HOME/.cache/cranpose/tmp"
}

ensure_local_temp_root() {
    local temp_root

    temp_root="$(local_temp_root)"
    if mkdir -p "$temp_root" >/dev/null 2>&1; then
        printf '%s\n' "$temp_root"
        return 0
    fi

    temp_root="$PWD/.cranpose-tmp"
    mkdir -p "$temp_root" >/dev/null 2>&1 || return 1
    printf '%s\n' "$temp_root"
}

enable_local_tmpdir() {
    local temp_root

    if is_ci_env; then
        return 0
    fi
    if [ "${CRANPOSE_USE_LOCAL_TMPDIR:-1}" = "0" ]; then
        return 0
    fi
    if [ -n "${TMPDIR:-}" ] && [ "$TMPDIR" != "/tmp" ]; then
        return 0
    fi

    temp_root="$(ensure_local_temp_root)"
    export TMPDIR="$temp_root"
}

create_local_temp_dir() {
    local prefix="${1:-cranpose}"
    local temp_root

    enable_local_tmpdir
    temp_root="${TMPDIR:-$(ensure_local_temp_root)}"
    if [ -n "$temp_root" ]; then
        local temp_dir
        temp_dir="$(mktemp -d "$temp_root/${prefix}.XXXXXX" 2>/dev/null || true)"
        if [ -n "$temp_dir" ]; then
            printf '%s\n' "$temp_dir"
            return 0
        fi
    fi

    temp_root="$PWD/.cranpose-tmp"
    mkdir -p "$temp_root" >/dev/null 2>&1 || return 1
    mktemp -d "$temp_root/${prefix}.XXXXXX"
}

local_cargo_build_jobs_default() {
    printf '1\n'
}

enable_local_cargo_job_limit() {
    if is_ci_env; then
        return 0
    fi
    if [ "${CRANPOSE_LIMIT_BUILD_JOBS:-1}" = "0" ]; then
        return 0
    fi
    if [ -n "${CARGO_BUILD_JOBS:-}" ]; then
        return 0
    fi

    export CARGO_BUILD_JOBS="${CRANPOSE_BUILD_JOBS:-$(local_cargo_build_jobs_default)}"
}

local_gradle_user_home() {
    if [ -n "${CRANPOSE_GRADLE_USER_HOME:-}" ]; then
        printf '%s\n' "$CRANPOSE_GRADLE_USER_HOME"
        return 0
    fi

    if [ -n "${XDG_CACHE_HOME:-}" ]; then
        printf '%s\n' "$XDG_CACHE_HOME/gradle"
        return 0
    fi

    printf '%s\n' "$HOME/.cache/gradle"
}

enable_local_gradle_home() {
    local gradle_home

    if is_ci_env; then
        return 0
    fi
    if [ "${CRANPOSE_USE_LOCAL_GRADLE_HOME:-1}" = "0" ]; then
        return 0
    fi
    if [ -n "${GRADLE_USER_HOME:-}" ]; then
        return 0
    fi

    gradle_home="$(local_gradle_user_home)"
    mkdir -p "$gradle_home" >/dev/null 2>&1 || return 1
    export GRADLE_USER_HOME="$gradle_home"
}

host_cpu_min_mhz() {
    awk '
        /cpu MHz/ {
            mhz = $4
            if (count == 0 || mhz < min) {
                min = mhz
            }
            count++
        }
        END {
            if (count > 0) {
                printf "%.0f\n", min
            }
        }
    ' /proc/cpuinfo 2>/dev/null || true
}

host_cpu_freq_summary() {
    awk '
        /cpu MHz/ {
            mhz = $4
            if (count == 0 || mhz < min) {
                min = mhz
            }
            if (mhz > max) {
                max = mhz
            }
            sum += mhz
            count++
        }
        END {
            if (count > 0) {
                printf "cpu_mhz=min:%.0f avg:%.0f max:%.0f", min, sum / count, max
            } else {
                printf "cpu_mhz=unknown"
            }
        }
    ' /proc/cpuinfo 2>/dev/null || printf 'cpu_mhz=unknown'
}

host_max_temp_c() {
    if ! command -v sensors >/dev/null 2>&1; then
        return 0
    fi

    sensors 2>/dev/null | awk '
        match($0, /:[[:space:]]*\+?([0-9]+(\.[0-9]+)?)[^0-9]*C/, parts) {
            temp = parts[1] + 0
            if (count == 0 || temp > max) {
                max = temp
            }
            count++
        }
        END {
            if (count > 0) {
                printf "%.1f\n", max
            }
        }
    '
}

host_cpu_count() {
    if command -v nproc >/dev/null 2>&1; then
        nproc
    elif command -v sysctl >/dev/null 2>&1; then
        sysctl -n hw.ncpu 2>/dev/null || echo 1
    else
        echo 1
    fi
}

# The one-minute load average, or empty where the kernel does not publish one.
host_load_1m() {
    if [ -r /proc/loadavg ]; then
        awk '{ printf "%.2f\n", $1 }' /proc/loadavg
    elif command -v sysctl >/dev/null 2>&1; then
        sysctl -n vm.loadavg 2>/dev/null | awk '{ gsub(/[{}]/, ""); printf "%.2f\n", $1 }'
    fi
}

host_state_summary() {
    local temp load
    temp="$(host_max_temp_c)"
    load="$(host_load_1m)"
    if [ -n "$temp" ]; then
        printf '%s temp_c=max:%s' "$(host_cpu_freq_summary)" "$temp"
    else
        printf '%s temp_c=unknown' "$(host_cpu_freq_summary)"
    fi
    if [ -n "$load" ]; then
        printf ' load_1m=%s/%s' "$load" "$(host_cpu_count)"
    fi
}

number_lt() {
    awk -v left="$1" -v right="$2" 'BEGIN { exit !(left < right) }'
}

number_gt() {
    awk -v left="$1" -v right="$2" 'BEGIN { exit !(left > right) }'
}

# Waits until the machine is quiet enough for a timing measurement to mean
# something, then returns.
#
# This runs in CI, unlike `wait_for_host_capacity`, because CI is where the
# problem is. `with_host_lock.sh` keeps this fleet's builds off the machine
# while the suite measures, but the same box also carries nineteen other
# repositories' runners, and none of them know the robot suite exists. A Rust build next door
# saturates twelve cores, the frame the suite is timing takes twice as long,
# and the suite reports a per-frame regression that reproduces nowhere -- the
# text-handle cycle test failed on `main` at `drag work_avg_ms 0.73 -> 1.66`
# and then passed on the same host, on the same commit AND on the commit
# before it, with nothing else running.
#
# It never refuses to run. A gate that will not start is worse than one that
# starts late, so after `CRANPOSE_HOST_QUIET_MAX_WAIT_SECS` it goes ahead and
# says on stdout that it did -- which puts the host's load in the log above
# any failure it then reports, so a red is attributable instead of mysterious.
wait_for_host_quiet() {
    local label="${1:-the timing suite}"

    if [ "${CRANPOSE_HOST_QUIET_GUARD:-1}" = "0" ]; then
        return 0
    fi

    local load cpus allowed elapsed
    load="$(host_load_1m)"
    if [ -z "$load" ]; then
        return 0
    fi

    cpus="$(host_cpu_count)"
    # Per core, so the same number means the same thing on a 4-core laptop and
    # a 12-core runner. At 0.6 the machine still has room for the suite's own
    # process; at 1.0 it is already fully committed to somebody else's build.
    allowed="$(awk -v cpus="$cpus" -v per="${CRANPOSE_HOST_MAX_LOAD_PER_CPU:-0.6}" \
        'BEGIN { printf "%.2f", cpus * per }')"
    local poll_secs="${CRANPOSE_HOST_QUIET_POLL_SECS:-15}"
    local max_wait_secs="${CRANPOSE_HOST_QUIET_MAX_WAIT_SECS:-600}"
    elapsed=0

    while number_gt "$load" "$allowed"; do
        if [ "$elapsed" -ge "$max_wait_secs" ]; then
            echo "host quiet guard: gave up after ${elapsed}s and started $label" \
                 "on a busy host (load_1m=$load over $allowed on $cpus cpus)." \
                 "TREAT ANY TIMING FAILURE BELOW AS UNMEASURED."
            return 0
        fi
        echo "host quiet guard: waiting for $label -- load_1m=$load over $allowed on $cpus cpus"
        sleep "$poll_secs"
        elapsed=$((elapsed + poll_secs))
        load="$(host_load_1m)"
        [ -n "$load" ] || return 0
    done

    if [ "$elapsed" -gt 0 ]; then
        echo "host quiet guard: host settled after ${elapsed}s (load_1m=$load of $allowed on $cpus cpus)"
    fi
    return 0
}

wait_for_host_capacity() {
    local label="${1:-work}"

    if is_ci_env; then
        return 0
    fi
    if [ "${CRANPOSE_HOST_GUARD:-1}" = "0" ]; then
        return 0
    fi

    # Frequency-scaled CPUs commonly idle below 1GHz and boost as soon as work
    # starts. A minimum is therefore opt-in; temperature remains the default
    # capacity guard.
    local min_cpu_mhz="${CRANPOSE_HOST_MIN_CPU_MHZ:-0}"
    local max_temp_c="${CRANPOSE_HOST_MAX_TEMP_C:-95}"
    local resume_temp_c="${CRANPOSE_HOST_RESUME_TEMP_C:-94}"
    local cooldown_secs="${CRANPOSE_HOST_COOLDOWN_SECS:-10}"
    local max_wait_secs="${CRANPOSE_HOST_MAX_WAIT_SECS:-900}"
    local elapsed=0
    local waiting_for_temp=0

    while true; do
        local min_mhz max_temp unhealthy reason
        min_mhz="$(host_cpu_min_mhz)"
        max_temp="$(host_max_temp_c)"
        unhealthy=0
        reason=""

        if number_gt "$min_cpu_mhz" "0" \
            && [ -n "$min_mhz" ] \
            && number_lt "$min_mhz" "$min_cpu_mhz"; then
            echo "host capacity guard refusing $label: cpu below ${min_cpu_mhz}MHz; $(host_state_summary)" >&2
            return 1
        fi

        if [ -n "$max_temp" ] && number_gt "$max_temp" "$max_temp_c"; then
            waiting_for_temp=1
        fi
        if [ "$waiting_for_temp" = "1" ]; then
            if [ -n "$max_temp" ] && number_gt "$max_temp" "$resume_temp_c"; then
                unhealthy=1
                reason="${reason:+$reason, }temperature above ${resume_temp_c}C resume"
            else
                waiting_for_temp=0
            fi
        fi

        if [ "$unhealthy" = "0" ]; then
            return 0
        fi

        if [ "$elapsed" -ge "$max_wait_secs" ]; then
            echo "host capacity guard timed out before $label after ${elapsed}s: $reason; $(host_state_summary)" >&2
            return 1
        fi

        echo "host capacity guard cooling before $label: $reason; $(host_state_summary)" >&2
        sleep "$cooldown_secs"
        elapsed=$((elapsed + cooldown_secs))
    done
}

# --- host capacity lock: shared (build) / exclusive (measurement) ----------
#
# The single implementation of both halves of this lock. scripts/ci/with_host_lock.sh
# and run_robot_test.sh's own two-phase acquisition both call these instead of
# each hand-rolling flock logic: a second copy is exactly the kind of drift
# that let the two implementations diverge in the first place.
#
# flock(2) on Linux only ever checks locks *currently held*, never ones
# merely queued, so a shared request that arrives while an exclusive request
# is still waiting is granted immediately, ahead of it, every time. A
# continuous stream of new builds can therefore starve a measurement's
# exclusive acquire forever. Confirmed on samarch-1 (util-linux flock
# 2.42.2): fifteen seconds of overlapping shared holders, a new one arriving
# every 500ms and each held 1.5s, kept a waiting exclusive acquirer out for
# the entire fifteen seconds -- it was granted only once the stream stopped.
#
# The fix is a turnstile: a second, plain mutex lock file every acquirer --
# reader or writer -- passes through before touching the real lock. A reader
# taps it and lets go. A writer holds it from the moment it starts waiting
# for the real lock until the moment it gets it, so no new reader can even
# begin waiting on the real lock during that window; the writer's wait is
# then bounded by the readers already in flight, not by how long new ones
# keep arriving. Exclusive-against-exclusive contention on the turnstile
# itself does not reproduce the same starvation -- the same experiment run
# against a continuous stream of quick exclusive turnstile taps let a
# mid-stream exclusive waiter in within ~2.7s of joining, an order of
# magnitude under the stream's own length -- so the turnstile needs no
# turnstile of its own.
readonly HOST_CAPACITY_LOCK_FILE="/tmp/cranpose-host-capacity.lock"
readonly HOST_CAPACITY_TURNSTILE_FILE="/tmp/cranpose-host-capacity.turnstile.lock"

host_capacity_lock_available() {
    command -v flock >/dev/null 2>&1 \
        && : >>"$HOST_CAPACITY_LOCK_FILE" 2>/dev/null \
        && : >>"$HOST_CAPACITY_TURNSTILE_FILE" 2>/dev/null
}

# Reader side: fd 7, tapped and released immediately. Call this before
# requesting the real lock in shared mode.
host_capacity_turnstile_pass() {
    exec 7>"$HOST_CAPACITY_TURNSTILE_FILE"
    flock -x 7
    exec 7>&-
}

# Writer side: fd 7, held open on return. Call this before requesting the
# real lock in exclusive mode, and call host_capacity_turnstile_release once
# that exclusive acquire succeeds.
host_capacity_turnstile_hold() {
    exec 7>"$HOST_CAPACITY_TURNSTILE_FILE"
    flock -x 7
}

host_capacity_turnstile_release() {
    exec 7>&-
}

# Acquires flock mode $2 (-s or -x) on already-open fd $1: non-blocking
# first, then blocking up to $3 seconds, narrating both attempts the same
# way for every caller.
#
# On timeout, the two modes disagree on purpose. A build's correctness never
# depended on exclusivity, only its neighbours' *timing* did, so --shared
# gives up and runs anyway ($5 = 0): a gate that will not start is worse
# than one that starts late. A measurement's correctness *is* the isolation,
# so --exclusive fails instead ($5 = 1): running the measurement beside a
# build is the exact bug this lock exists to prevent, and a clearly-failed
# gate beats a silently-wrong number that gets diagnosed as a regression.
host_capacity_flock_wait() {
    local fd="$1" flock_mode="$2" max_wait_secs="$3" label="$4" fail_on_timeout="${5:-0}"

    if flock -n "$flock_mode" "$fd"; then
        echo "host lock: took $label immediately"
        return 0
    fi

    echo "host lock: busy -- waiting for $label..."
    local started_at
    started_at=$(date +%s)
    if flock -w "$max_wait_secs" "$flock_mode" "$fd"; then
        echo "host lock: took $label after $(( $(date +%s) - started_at ))s"
        return 0
    fi

    if [ "$fail_on_timeout" = "1" ]; then
        echo "host lock: gave up waiting for $label after ${max_wait_secs}s." \
             "Refusing to measure beside a build instead of reporting an unmeasured number." >&2
        return 1
    fi

    echo "host lock: gave up waiting for $label after ${max_wait_secs}s" \
         "and started anyway. TREAT ANY TIMING RESULT BELOW AS UNMEASURED."
    return 0
}
