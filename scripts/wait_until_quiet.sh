#!/usr/bin/env bash
# Wait until nothing on this host matches any of the given patterns.
#
# Hand-rolled `until ! pgrep -f "$pat"; do sleep; done` loops do not work, and
# the way they fail is silent. `pgrep -f` matches against full command lines,
# and the command line of the shell running the loop contains the pattern --
# so the loop matches itself, the condition never goes false, and it waits
# forever while looking exactly like patience. Four of these were found alive
# on one build host in a single day, from three separate callers; the oldest
# had been spinning for twenty hours and four minutes, orphaned to init, with
# nothing left to read the output it was never going to produce.
#
# Two things here make that impossible rather than merely documented. Each
# pattern is bracketed before use, so it cannot match its own literal text.
# And the wait is bounded: it always ends, and a timeout exits non-zero with
# the survivors named, so a caller cannot mistake "gave up" for "quiet".
set -euo pipefail

timeout_seconds=1800
interval_seconds=15
patterns=()

usage() {
    cat >&2 <<'USAGE'
usage: wait_until_quiet.sh [--timeout SECONDS] [--interval SECONDS] PATTERN...

Waits until no process matches any PATTERN, then exits 0. Exits 2 if the
timeout passes while something still matches, naming what survived.

Patterns are matched against full command lines, like `pgrep -f`.
USAGE
}

while [ $# -gt 0 ]; do
    case "$1" in
        --timeout) timeout_seconds="${2:?--timeout needs a value}"; shift 2 ;;
        --interval) interval_seconds="${2:?--interval needs a value}"; shift 2 ;;
        -h|--help) usage; exit 0 ;;
        --) shift; while [ $# -gt 0 ]; do patterns+=("$1"); shift; done ;;
        -*) echo "unknown option: $1" >&2; usage; exit 64 ;;
        *) patterns+=("$1"); shift ;;
    esac
done

if [ "${#patterns[@]}" -eq 0 ]; then
    usage
    exit 64
fi

# The bracket trick alone is NOT enough here, and the reason is worth stating.
# `[c]argo build` stops a pattern matching its own literal text, which is what
# saves an inline `until ! pgrep -f "cargo build"` loop. But this script takes
# the pattern as an ARGUMENT, so its own argv holds the unbracketed string --
# and `[c]argo build` matches `cargo build` perfectly. Every subshell this
# script forks inherits that argv and answers the pgrep. So the bracket stays,
# because it costs nothing and helps callers who inline a literal, and the
# thing actually doing the work is the ancestry filter below.
self_excluding() {
    local pattern="$1" i char
    for (( i = 0; i < ${#pattern}; i++ )); do
        char="${pattern:i:1}"
        case "$char" in
            [A-Za-z0-9_])
                printf '%s[%s]%s\n' "${pattern:0:i}" "$char" "${pattern:i+1}"
                return 0
                ;;
        esac
    done
    printf '%s\n' "$pattern"
}

# True when pid is this script or anything it forked. Walks the parent chain
# rather than comparing process groups: a caller that backgrounds the process
# being waited for shares this script's group, so a group filter would discard
# the very thing the wait is for.
own_descendant() {
    local pid="$1" hops=0
    while [ -n "$pid" ] && [ "$pid" != "1" ] && [ "$hops" -lt 40 ]; do
        [ "$pid" = "$$" ] && return 0
        pid="$(ps -o ppid= -p "$pid" 2>/dev/null | tr -d ' ')"
        hops=$(( hops + 1 ))
    done
    return 1
}

# A pid pgrep reports is not necessarily a process still doing something. An
# exited child whose parent has not reaped it keeps its full command line until
# the wait(2), so a waiter that trusts pgrep can block on a process that
# finished minutes ago.
alive() {
    local pid state
    while read -r pid; do
        if [ -z "$pid" ] || own_descendant "$pid"; then
            continue
        fi
        state="$(ps -o state= -p "$pid" 2>/dev/null || true)"
        case "${state// /}" in
            ''|Z*) continue ;;
        esac
        printf '%s\n' "$pid"
    done
}

survivors() {
    local pattern matches out=""
    for pattern in "${patterns[@]}"; do
        matches="$(pgrep -f "$(self_excluding "$pattern")" 2>/dev/null | alive || true)"
        if [ -n "${matches//[[:space:]]/}" ]; then
            out+="  $pattern: $(printf '%s' "$matches" | tr '\n' ' ')"$'\n'
        fi
    done
    printf '%s' "$out"
}

deadline=$(( SECONDS + timeout_seconds ))
while :; do
    still="$(survivors)"
    if [ -z "$still" ]; then
        echo "quiet: nothing matches ${patterns[*]}"
        exit 0
    fi
    if [ "$SECONDS" -ge "$deadline" ]; then
        echo "timed out after ${timeout_seconds}s; still running:" >&2
        printf '%s' "$still" >&2
        exit 2
    fi
    sleep "$interval_seconds"
done
