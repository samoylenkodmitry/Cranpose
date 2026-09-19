#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'USAGE'
usage: nightly_should_run.sh --repo <owner/repo> --sha <sha> --run-id <id>
                             [--window-hours <n>] [--force]

Prints `run=true` or `run=false` on stdout for $GITHUB_OUTPUT, and says why on
stderr.

The nightly board has two triggers: GitHub's schedule, which is free and
sometimes silently late or dropped, and a dispatch from samarch-1's crontab,
which is reliable while that box is up. Either alone leaves a hole; both
together would run the whole suite twice on the two Linux slots. This answers
"has tonight's nightly already happened", so whichever trigger arrives second
becomes a fifteen-second no-op.

A failed run does not count: a retry is exactly what should happen. `--force`
is for a human who wants the board rerun regardless.
USAGE
}

repo=""
sha=""
run_id=""
window_hours=20
force=0
while [ "$#" -gt 0 ]; do
  case "$1" in
    --repo) [ "$#" -ge 2 ] || { usage; exit 2; }; repo="$2"; shift 2 ;;
    --sha) [ "$#" -ge 2 ] || { usage; exit 2; }; sha="$2"; shift 2 ;;
    --run-id) [ "$#" -ge 2 ] || { usage; exit 2; }; run_id="$2"; shift 2 ;;
    --window-hours) [ "$#" -ge 2 ] || { usage; exit 2; }; window_hours="$2"; shift 2 ;;
    --force) force=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) usage; exit 2 ;;
  esac
done

if [ -z "$repo" ] || [ -z "$sha" ] || [ -z "$run_id" ]; then
  usage
  exit 2
fi

if [ "$force" -eq 1 ]; then
  echo "nightly_should_run: forced, running regardless of what already ran" >&2
  echo "run=true"
  exit 0
fi

runs_json=$(gh api -H "Accept: application/vnd.github+json" \
  "repos/$repo/actions/workflows/nightly.yml/runs?per_page=100")

verdict=$(printf '%s' "$runs_json" | python3 -c '
import datetime
import json
import sys

payload = json.load(sys.stdin)
runs = payload["workflow_runs"] if isinstance(payload, dict) else payload
sha, run_id, window_hours = sys.argv[1], sys.argv[2], float(sys.argv[3])
now = datetime.datetime.now(datetime.timezone.utc)

answer = "true"
reason = "nothing has covered this commit tonight"
for run in runs:
    other = str(run.get("id"))
    if other == str(run_id):
        continue
    if run.get("head_sha") != sha:
        continue
    created = run.get("created_at")
    if not created:
        continue
    started = datetime.datetime.fromisoformat(created.replace("Z", "+00:00"))
    if (now - started).total_seconds() > window_hours * 3600:
        continue
    status = run.get("status")
    if status != "completed":
        answer = "false"
        reason = "run " + other + " is already " + str(status) + " on this commit"
        break
    if run.get("conclusion") == "success":
        answer = "false"
        reason = "run " + other + " already succeeded on this commit"
        break

sys.stdout.write(answer + "\t" + reason + "\n")
' "$sha" "$run_id" "$window_hours")

answer="${verdict%%	*}"
reason="${verdict#*	}"
echo "nightly_should_run: $reason" >&2
echo "run=$answer"
