#!/usr/bin/env bash
set -euo pipefail

# The nightly board is triggered twice on purpose -- GitHub's schedule, which
# was silently 17 minutes late and still absent on the night this was written,
# and samarch-1's crontab, which is reliable while that box is up. This decides
# which arrival does the work and which one is a no-op, so getting it wrong
# either runs the 30-minute suite twice on the two Linux slots, or skips the
# night entirely.

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SCRIPT="$REPO_ROOT/scripts/ci/nightly_should_run.sh"

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

work_dir="$(mktemp -d)"
trap 'rm -r -- "$work_dir" 2>/dev/null || true' EXIT

bin_dir="$work_dir/bin"
mkdir -p "$bin_dir"
runs_file="$work_dir/runs.json"
jobs_file="$work_dir/jobs.json"
calls_file="$work_dir/calls"

cat > "$bin_dir/gh" <<'GH'
#!/usr/bin/env bash
printf '%s\n' "$*" >> "$NIGHTLY_TEST_CALLS"
case "$*" in
    */jobs*) cat "$NIGHTLY_TEST_JOBS" ;;
    *) cat "$NIGHTLY_TEST_RUNS" ;;
esac
GH
chmod +x "$bin_dir/gh"

export NIGHTLY_TEST_CALLS="$calls_file"
export NIGHTLY_TEST_RUNS="$runs_file"
export NIGHTLY_TEST_JOBS="$jobs_file"

# Every earlier run below did the night's work, unless a check says otherwise.
worked='{"jobs": [{"conclusion": "success"}, {"conclusion": "success"}]}'
printf '%s\n' "$worked" > "$jobs_file"

SHA=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
OTHER=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
SELF=999

recent() { python3 -c "
import datetime,sys
print((datetime.datetime.now(datetime.timezone.utc)-datetime.timedelta(hours=float(sys.argv[1]))).strftime('%Y-%m-%dT%H:%M:%SZ'))
" "$1"; }

verdict() {
    : > "$calls_file"
    PATH="$bin_dir:$PATH" bash "$SCRIPT" --repo owner/repo --sha "$SHA" --run-id "$SELF" "$@" \
        2> "$work_dir/why"
}

# Nothing has run tonight.
printf '{"workflow_runs": []}\n' > "$runs_file"
out="$(verdict)"
check "an empty history runs the board" test "$out" = "run=true"
check "the listing call really happened, so the checks below read a live instrument" \
    grep -q "workflows/nightly.yml/runs" "$calls_file"

# Tonight's run already succeeded on this commit.
cat > "$runs_file" <<JSON
{"workflow_runs": [
  {"id": 1, "head_sha": "$SHA", "status": "completed", "conclusion": "success", "created_at": "$(recent 2)"}
]}
JSON
out="$(verdict)"
check "a nightly that already succeeded tonight stops a second one" test "$out" = "run=false"
check "and says which run covered it" grep -q "already succeeded" "$work_dir/why"

# One is running right now.
cat > "$runs_file" <<JSON
{"workflow_runs": [
  {"id": 2, "head_sha": "$SHA", "status": "in_progress", "conclusion": null, "created_at": "$(recent 0.2)"}
]}
JSON
out="$(verdict)"
check "a nightly already in progress stops a second one" test "$out" = "run=false"

# A failure is exactly what should be retried.
cat > "$runs_file" <<JSON
{"workflow_runs": [
  {"id": 3, "head_sha": "$SHA", "status": "completed", "conclusion": "failure", "created_at": "$(recent 2)"}
]}
JSON
out="$(verdict)"
check "a failed nightly is retried rather than treated as done" test "$out" = "run=true"

# A different commit is a different night's work.
cat > "$runs_file" <<JSON
{"workflow_runs": [
  {"id": 4, "head_sha": "$OTHER", "status": "completed", "conclusion": "success", "created_at": "$(recent 1)"}
]}
JSON
out="$(verdict)"
check "a success on another commit does not count as covering this one" test "$out" = "run=true"

# Yesterday's success must not suppress tonight.
cat > "$runs_file" <<JSON
{"workflow_runs": [
  {"id": 5, "head_sha": "$SHA", "status": "completed", "conclusion": "success", "created_at": "$(recent 30)"}
]}
JSON
out="$(verdict)"
check "a success older than the window does not suppress tonight" test "$out" = "run=true"

# A run must never suppress itself.
cat > "$runs_file" <<JSON
{"workflow_runs": [
  {"id": $SELF, "head_sha": "$SHA", "status": "in_progress", "conclusion": null, "created_at": "$(recent 0.1)"}
]}
JSON
out="$(verdict)"
check "a run does not mistake itself for a duplicate" test "$out" = "run=true"

# Force wins, and does not even ask.
cat > "$runs_file" <<JSON
{"workflow_runs": [
  {"id": 6, "head_sha": "$SHA", "status": "completed", "conclusion": "success", "created_at": "$(recent 1)"}
]}
JSON
out="$(verdict --force)"
check "a forced run happens even when tonight is already covered" test "$out" = "run=true"
check "and a forced run does not call the api at all" \
    bash -c '! [ -s "$1" ]' _ "$calls_file"

# GitHub's schedule arrives hours after the crontab dispatch, on a main that moved.
cat > "$runs_file" <<JSON
{"workflow_runs": [
  {"id": 7, "head_sha": "$OTHER", "status": "completed", "conclusion": "success", "created_at": "$(recent 8)"}
]}
JSON
out="$(verdict --event schedule)"
check "a late scheduled run stops once tonight's dispatch succeeded on another commit" \
    test "$out" = "run=false"

cat > "$runs_file" <<JSON
{"workflow_runs": [
  {"id": 8, "head_sha": "$OTHER", "status": "completed", "conclusion": "success", "created_at": "$(recent 14)"}
]}
JSON
out="$(verdict --event schedule)"
check "a scheduled run does not take yesterday's daytime run for tonight's" test "$out" = "run=true"

cat > "$runs_file" <<JSON
{"workflow_runs": [
  {"id": 9, "head_sha": "$OTHER", "status": "completed", "conclusion": "failure", "created_at": "$(recent 7)"}
]}
JSON
out="$(verdict --event schedule)"
check "a scheduled run retries after tonight's dispatch failed" test "$out" = "run=true"

# A run that only decided to skip succeeded without testing anything.
cat > "$runs_file" <<JSON
{"workflow_runs": [
  {"id": 10, "head_sha": "$SHA", "status": "completed", "conclusion": "success", "created_at": "$(recent 2)"}
]}
JSON
printf '%s\n' '{"jobs": [{"conclusion": "success"}, {"conclusion": "skipped"}, {"conclusion": "skipped"}]}' > "$jobs_file"
out="$(verdict)"
printf '%s\n' "$worked" > "$jobs_file"
check "a run that only decided to skip does not cover the night" test "$out" = "run=true"

# A run queued behind this one asks the same question once this one ends.
cat > "$runs_file" <<JSON
{"workflow_runs": [
  {"id": $((SELF + 1)), "head_sha": "$SHA", "status": "queued", "conclusion": null, "created_at": "$(recent 0.1)"}
]}
JSON
out="$(verdict)"
check "a run that arrived later does not stop this one" test "$out" = "run=true"

printf '{"workflow_runs": []}\n' > "$runs_file"
out="$(verdict --workflow perf-nightly.yml)"
check "the history read is the one of the workflow named" \
    grep -q "workflows/perf-nightly.yml/runs" "$calls_file"

status=0
PATH="$bin_dir:$PATH" bash "$SCRIPT" --repo owner/repo --sha "$SHA" > /dev/null 2>&1 || status=$?
check "a missing run id is rejected instead of guessed" test "$status" -eq 2

if [ "$failures" -ne 0 ]; then
    echo "nightly_should_run_test: $failures check(s) failed" >&2
    exit 1
fi
echo "nightly_should_run_test: all checks passed"
