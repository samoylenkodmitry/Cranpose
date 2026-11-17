#!/usr/bin/env bash
# The verdicts of steady_ledger.sh's comparison: the nightly fails only when a
# value rises past its budget plus its spread (and 2% of the budget for
# copied bytes), or a row and its budget do not match up. Also the sums of
# valgrind's allocator trace, which give the bytes that `realloc` copies.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

failures=0
work_dir="$(mktemp -d)"
trap 'rm -r -- "$work_dir"' EXIT

cat > "$work_dir/budget.txt" <<'BUDGET'
wall-t20 malloc 2.00 0.00
wall-t20 copied 31556 10
wall-t20-start group:node 4.00 0.02
dashboard malloc 33.90 0.50
BUDGET
cat > "$work_dir/groups.txt" <<'GROUPS'
node 1.0 new_with_virtual.* @ crates/cranpose-ui/
GROUPS

check() {
    local description=$1 status=$2 pattern=$3 output=$4 actual=$5
    if [ "$actual" -eq "$status" ] && grep -q -- "$pattern" <<< "$output"; then
        echo "ok: $description"
    else
        echo "FAIL: $description (exit $actual)" >&2
        echo "$output" >&2
        failures=$((failures + 1))
    fi
}

# expect DESCRIPTION STATUS PATTERN ROWS: the comparison of ROWS with the
# budget exits with STATUS and prints PATTERN.
expect() {
    local output actual=0
    printf '%s\n' "$4" > "$work_dir/rows.txt"
    output=$("$HERE/steady_ledger.sh" --compare "$work_dir/rows.txt" "$work_dir/budget.txt" \
        "$work_dir/groups.txt") || actual=$?
    check "$1" "$2" "$3" "$output" "$actual"
}

at_budget="wall-t20 malloc 2.00
wall-t20 copied 31556
wall-t20-start group:node 4.00
dashboard malloc 33.90"

expect "values at their budgets pass" 0 "wall-t20 .* ok" "$at_budget"
expect "a rise inside the spread passes" 0 "dashboard .* ok" "${at_budget/33.90/34.30}"
expect "a rise past the spread fails" 1 "dashboard .* ROSE malloc" "${at_budget/33.90/34.50}"
# Copied bytes have 2% of their budget on top of their spread: 31556 + 10 + 631.
expect "copied bytes inside 2% past the spread pass" 0 "wall-t20 .* ok" "${at_budget/31556/32197}"
expect "copied bytes past 2% beyond the spread fail" 1 "wall-t20 .* ROSE copied" "${at_budget/31556/32198}"
expect "a group that rises fails" 1 "wall-t20-start .* ROSE group:node" "${at_budget/4.00/4.10}"
expect "the start row prints its groups with their targets" 0 "node 4.00 \[1\]" "$at_budget"
expect "a fall passes and names the value" 0 "wall-t20 .* fell malloc" \
    "${at_budget/2.00/1.00}"
expect "a value with no budget fails" 1 "feed .* ROSE malloc (no budget)" "$at_budget
feed malloc 500.00"
expect "a budget with no value fails" 1 "dashboard .*missing from the run: malloc" \
    "$(head -3 <<< "$at_budget")"

# valgrind's `--trace-malloc=yes` lines: a `realloc` keeps the smaller of the
# old and the new size, also for a block of an aligned allocation, whose
# address comes on a line of its own, and for a block whose address comes
# after another thread's call; `free(0x0)` frees nothing; a phase line can
# land inside a call's line.
trace=$(python3 "$HERE/steady_ledger.py" trace <<'TRACE'
--7-- malloc(100) = 0x10
steady_frames: phase steady
--7-- posix_memalign(al 128, size 512)
--7--  = 0x80
--7-- realloc(0x10,40) = 0x20
--7-- realloc(0x80,1024) = 0x90
--7-- realloc(0x0,8) = 0xA0
--7-- calloc(4,8) = 0xB0
--7-- free(0x0)
--7-- free(0x20)
--7-- malloc(64)free(0x0)
--7--  = 0xC0
--7-- realloc(0xC0,10) = 0xD0
--7-- malloc(8)steady_frames: phase end
--7--  = 0xE0
==7== Total: 99 bytes in 1 blocks
TRACE
)
check "the trace sums each phase and keeps other lines" 0 \
    "steady_ledger trace steady fresh=4 realloc=4 free=1 realloc_bytes=562" "$trace" 0
check "the trace passes valgrind's own lines through" 0 "Total: 99 bytes" "$trace" 0

# An A/B archives the whole repository, from a copy of the script three
# levels down and run from a subdirectory: the archive holds the top's
# Cargo.toml and only then lacks the bench app's example, so the base build
# stops at once.
repo="$work_dir/repo"
mkdir -p "$repo/a/b/c"
touch "$repo/Cargo.toml"
cp "$HERE"/steady_ledger.sh "$HERE"/steady_ledger.py "$repo/a/b/c/"
git -C "$repo" init -q
git -C "$repo" add .
git -C "$repo" -c user.name=test -c user.email=test@example.com -c core.hooksPath=/dev/null commit -q -m tree
actual=0
output=$(cd "$repo/a/b" && STEADY_LEDGER_SHARED="$work_dir/shared" bash c/steady_ledger.sh \
    --dir "$work_dir/run" --base HEAD 2>&1) || actual=$?
check "an A/B archives the repository's top from a subdirectory" 2 "holds no steady_frames example" \
    "$output" "$actual"

if [ "$failures" -gt 0 ]; then
    echo "$failures steady ledger check(s) failed" >&2
    exit 1
fi
