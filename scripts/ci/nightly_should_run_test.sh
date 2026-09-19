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
calls_file="$work_dir/calls"

cat > "$bin_dir/gh" <<'GH'
#!/usr/bin/env bash
printf '%s\n' "$*" >> "$NIGHTLY_TEST_CALLS"
cat "$NIGHTLY_TEST_RUNS"
GH
chmod +x "$bin_dir/gh"

export NIGHTLY_TEST_CALLS="$calls_file"
export NIGHTLY_TEST_RUNS="$runs_file"

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

status=0
PATH="$bin_dir:$PATH" bash "$SCRIPT" --repo owner/repo --sha "$SHA" > /dev/null 2>&1 || status=$?
check "a missing run id is rejected instead of guessed" test "$status" -eq 2

if [ "$failures" -ne 0 ]; then
    echo "nightly_should_run_test: $failures check(s) failed" >&2
    exit 1
fi
echo "nightly_should_run_test: all checks passed"
