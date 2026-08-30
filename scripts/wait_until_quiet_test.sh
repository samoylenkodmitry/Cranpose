#!/usr/bin/env bash
# Regression test for wait_until_quiet.sh.
#
# The case that matters is the second one. A wait loop written with a bare
# `pgrep -f` matches the command line that carries the pattern, so it waits
# for itself and never finishes. That failure has no symptom: the process
# sits at almost no CPU, prints nothing, and is indistinguishable from a wait
# that is merely long. It is only visible if something asserts that a wait on
# a quiet host returns, which is what this file does.
#
# Every case is bounded. A test for a hang that can itself hang would be the
# same defect one level up.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
waiter="$script_dir/wait_until_quiet.sh"
workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT
failures=0

check() {
    local name="$1" expected="$2" actual="$3"
    if [ "$expected" = "$actual" ]; then
        echo "ok   $name"
    else
        echo "FAIL $name: expected exit $expected, got $actual"
        failures=$(( failures + 1 ))
    fi
}

# 1. Nothing matches: returns at once.
set +e
"$waiter" --timeout 5 --interval 1 "cranpose-no-such-process-aardvark" >/dev/null 2>&1
check "quiet host returns 0" 0 "$?"
set -e

# 2. The pattern appears in the waiter's own command line. A bare
#    `pgrep -f` loop matches itself here and waits forever; this must not.
#    The short timeout is the assertion: a self-match cannot finish inside it.
set +e
"$waiter" --timeout 5 --interval 1 "wait_until_quiet_marker_xyzzy" >/dev/null 2>&1
check "pattern in own argv does not match itself" 0 "$?"
set -e

# The sleeper is given a name nothing else on the host can carry. `sleep 3` was
# the obvious choice and it is wrong twice over: the string appears in the
# waiter's own argument list, and it is a prefix of `sleep 30`, so case 4 below
# would answer case 3's question.
sleeper="$workdir/quiet-test-sleeper-$$"
mkdir -p "$workdir"
printf '#!/bin/sh\nsleep "$1"\n' > "$sleeper"
chmod +x "$sleeper"

# 3. A real match blocks, then clears when the process ends.
"$sleeper" 3 &
sleep_pid=$!
start=$SECONDS
set +e
"$waiter" --timeout 20 --interval 1 "$(basename "$sleeper") 3" >/dev/null 2>&1
rc=$?
set -e
wait "$sleep_pid" 2>/dev/null || true
check "waits for a real match" 0 "$rc"
elapsed=$(( SECONDS - start ))
if [ "$elapsed" -lt 2 ] || [ "$elapsed" -ge 19 ]; then
    echo "FAIL waited for a real match: returned in ${elapsed}s (want 2..18)"
    failures=$(( failures + 1 ))
else
    echo "ok   waited for a real match (blocked ${elapsed}s)"
fi

# 4. A match that outlives the timeout exits 2 and names the survivor.
"$sleeper" 45 &
long_pid=$!
set +e
out="$("$waiter" --timeout 2 --interval 1 "$(basename "$sleeper") 45" 2>&1)"
rc=$?
set -e
kill "$long_pid" 2>/dev/null || true
wait "$long_pid" 2>/dev/null || true
check "timeout exits 2" 2 "$rc"
case "$out" in
    *"$(basename "$sleeper") 45"*) echo "ok   timeout names the survivor" ;;
    *) echo "FAIL timeout names the survivor: got $out"; failures=$(( failures + 1 )) ;;
esac

# 5. No pattern is a usage error, not a wait on everything.
set +e
"$waiter" >/dev/null 2>&1
check "no pattern exits 64" 64 "$?"
set -e

if [ "$failures" -ne 0 ]; then
    echo "$failures failure(s)"
    exit 1
fi
echo "wait_until_quiet: all cases pass"
