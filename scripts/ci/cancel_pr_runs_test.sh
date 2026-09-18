#!/usr/bin/env bash
set -euo pipefail

# Merging a pull request does not stop its pull_request runs. On five
# self-hosted runners those runs hold every slot while main waits, so the
# selection below decides whether a merge frees the board or blocks it. It must
# reach queued and running work at the merged head, and it must never touch the
# push run that main is waiting on.

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SCRIPT="$REPO_ROOT/scripts/ci/cancel_pr_runs.sh"

failures=0
check() {
    local description="$1"
    shift
    if "$@"; then
        echo "ok: $description"
    else
        echo "FAIL: $description" >&2
        failures=$((failures + 1))
    fi
}

not_matched() {
    ! grep -q "$1" "$2"
}

work_dir="$(mktemp -d)"
trap 'rm -r -- "$work_dir" 2>/dev/null || true' EXIT

bin_dir="$work_dir/bin"
mkdir -p "$bin_dir"
calls_file="$work_dir/calls"
runs_file="$work_dir/runs.json"

cat > "$bin_dir/gh" <<'GH'
#!/usr/bin/env bash
printf '%s\n' "$*" >> "$CANCEL_PR_RUNS_TEST_CALLS"
for arg in "$@"; do
    case "$arg" in
        *actions/runs/*/cancel)
            if [ -n "${CANCEL_PR_RUNS_TEST_CANCEL_FAILS:-}" ]; then
                echo "simulated cancel failure" >&2
                exit 1
            fi
            exit 0
            ;;
    esac
done
cat "$CANCEL_PR_RUNS_TEST_RUNS"
GH
chmod +x "$bin_dir/gh"

export CANCEL_PR_RUNS_TEST_CALLS="$calls_file"
export CANCEL_PR_RUNS_TEST_RUNS="$runs_file"

MERGED_SHA="1111111111111111111111111111111111111111"
OTHER_SHA="2222222222222222222222222222222222222222"

cat > "$runs_file" <<JSON
{"workflow_runs": [
  {"id": 101, "event": "pull_request", "status": "queued", "head_sha": "$MERGED_SHA"},
  {"id": 102, "event": "pull_request", "status": "in_progress", "head_sha": "$MERGED_SHA"},
  {"id": 103, "event": "pull_request", "status": "completed", "head_sha": "$MERGED_SHA"},
  {"id": 104, "event": "push", "status": "in_progress", "head_sha": "$MERGED_SHA"},
  {"id": 105, "event": "pull_request", "status": "in_progress", "head_sha": "$OTHER_SHA"}
]}
JSON

run_script() {
    : > "$calls_file"
    PATH="$bin_dir:$PATH" bash "$SCRIPT" --repo owner/repo --head-sha "$MERGED_SHA" \
        > "$work_dir/out" 2> "$work_dir/err"
}

status=0
run_script || status=$?
cancelled="$(grep -c 'actions/runs/[0-9]*/cancel' "$calls_file" || true)"

check "a merged pull request's runs are cancelled without error" test "$status" -eq 0
check "the list call really happened, so the assertions below read a live instrument" \
    grep -q 'actions/runs?event=pull_request' "$calls_file"
check "a queued run at the merged head is cancelled" \
    grep -q 'actions/runs/101/cancel' "$calls_file"
check "a running run at the merged head is cancelled" \
    grep -q 'actions/runs/102/cancel' "$calls_file"
check "an already finished run is left alone" \
    not_matched 'actions/runs/103/cancel' "$calls_file"
check "the push run main is waiting on is left alone" \
    not_matched 'actions/runs/104/cancel' "$calls_file"
check "another commit's run is left alone" \
    not_matched 'actions/runs/105/cancel' "$calls_file"
check "exactly the two superseded runs were cancelled" test "$cancelled" -eq 2


cat > "$runs_file" <<'JSON'
{"workflow_runs": []}
JSON
status=0
run_script || status=$?
check "an empty board is not a failure" test "$status" -eq 0
check "an empty board says so instead of staying silent" \
    grep -q 'nothing still running' "$work_dir/out"

cat > "$runs_file" <<JSON
{"workflow_runs": [
  {"id": 101, "event": "pull_request", "status": "queued", "head_sha": "$MERGED_SHA"}
]}
JSON
export CANCEL_PR_RUNS_TEST_CANCEL_FAILS=1
status=0
run_script || status=$?
unset CANCEL_PR_RUNS_TEST_CANCEL_FAILS
check "a runner left holding a slot is reported as a failure" test "$status" -ne 0

status=0
PATH="$bin_dir:$PATH" bash "$SCRIPT" --repo owner/repo > /dev/null 2>&1 || status=$?
check "a missing head commit is rejected instead of guessed" test "$status" -eq 2

status=0
PATH="$bin_dir:$PATH" bash "$SCRIPT" --head-sha "$MERGED_SHA" > /dev/null 2>&1 || status=$?
check "a missing repository is rejected instead of guessed" test "$status" -eq 2

if [ "$failures" -ne 0 ]; then
    echo "cancel_pr_runs_test: $failures check(s) failed" >&2
    exit 1
fi
echo "cancel_pr_runs_test: all checks passed"
