#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'USAGE'
usage: nightly_should_run.sh --repo <owner/repo> --sha <sha> --run-id <id>
                             [--workflow <file>] [--event <name>]
                             [--window-hours <n>] [--force]

Prints `run=true` or `run=false` on stdout for $GITHUB_OUTPUT, and says why on
stderr.

The nightly board has two triggers: GitHub's schedule, which is free and
sometimes silently late or dropped, and a dispatch from samarch-1's crontab,
which is reliable while that box is up. Either alone leaves a hole; both
together would run the whole suite twice on the two Linux slots. This answers
"has tonight's nightly already happened", so whichever trigger arrives second
becomes a fifteen-second no-op.

GitHub's schedule fires hours late: 7 to 8 hours after the crontab dispatch
in November 2026. By then main has usually moved, so a scheduled run (`--event
schedule`) counts a run of any commit from the last 12 hours. Any other
trigger counts only a run of its own commit, inside `--window-hours`.

A failed run does not count: a retry is exactly what should happen. Neither
does a run that only decided to skip, nor one that arrived after this one.
`--force` is for a human who wants the board rerun regardless.
USAGE
}

repo=""
sha=""
run_id=""
workflow="nightly.yml"
event=""
window_hours=20
# The schedule fires up to about 8 hours after the crontab dispatch.
schedule_window_hours=12
force=0
while [ "$#" -gt 0 ]; do
  case "$1" in
    --repo) [ "$#" -ge 2 ] || { usage; exit 2; }; repo="$2"; shift 2 ;;
    --sha) [ "$#" -ge 2 ] || { usage; exit 2; }; sha="$2"; shift 2 ;;
    --run-id) [ "$#" -ge 2 ] || { usage; exit 2; }; run_id="$2"; shift 2 ;;
    --workflow) [ "$#" -ge 2 ] || { usage; exit 2; }; workflow="$2"; shift 2 ;;
    --event) [ "$#" -ge 2 ] || { usage; exit 2; }; event="$2"; shift 2 ;;
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
  "repos/$repo/actions/workflows/$workflow/runs?per_page=100")

if [ "$event" = "schedule" ]; then
  any_commit=1
  window_hours="$schedule_window_hours"
else
  any_commit=0
fi

verdict=$(printf '%s' "$runs_json" | python3 -c '
import datetime
import json
import subprocess
import sys

payload = json.load(sys.stdin)
runs = payload["workflow_runs"] if isinstance(payload, dict) else payload
repo, sha, run_id = sys.argv[1], sys.argv[2], int(sys.argv[3])
window_hours, any_commit = float(sys.argv[4]), sys.argv[5] == "1"
now = datetime.datetime.now(datetime.timezone.utc)


def did_work(other):
    # A run that only decided to skip ran one job; a nightly runs more.
    listing = subprocess.run(
        ["gh", "api", "repos/" + repo + "/actions/runs/" + str(other) + "/jobs?per_page=100"],
        check=True, capture_output=True, text=True,
    ).stdout
    ran = [job for job in json.loads(listing)["jobs"]
           if job.get("conclusion") not in (None, "skipped")]
    return len(ran) > 1


answer = "true"
reason = "nothing has covered this commit tonight"
if any_commit:
    reason = "nothing has run tonight"
for run in runs:
    other = run.get("id")
    # A run that arrived later asks this same question once this one ends.
    if other is None or int(other) >= run_id:
        continue
    if not any_commit and run.get("head_sha") != sha:
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
        reason = "run " + str(other) + " is already " + str(status)
        break
    if run.get("conclusion") == "success" and did_work(other):
        answer = "false"
        reason = "run " + str(other) + " already succeeded on " + str(run.get("head_sha"))[:9]
        break

sys.stdout.write(answer + "\t" + reason + "\n")
' "$repo" "$sha" "$run_id" "$window_hours" "$any_commit")

answer="${verdict%%	*}"
reason="${verdict#*	}"
echo "nightly_should_run: $reason" >&2
echo "run=$answer"
