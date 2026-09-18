#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'USAGE'
usage: cancel_pr_runs.sh --repo <owner/repo> --head-sha <sha>

Cancels every workflow run still occupying a runner for a pull request head
commit that no longer needs one. GitHub keeps pull_request runs going after the
pull request is merged or closed, so on this repository's five self-hosted
runners a merged branch can hold every slot for the length of a robot build.
USAGE
}

repo=""
head_sha=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --repo)
      [ "$#" -ge 2 ] || { usage; exit 2; }
      repo="$2"
      shift 2
      ;;
    --head-sha)
      [ "$#" -ge 2 ] || { usage; exit 2; }
      head_sha="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      usage
      exit 2
      ;;
  esac
done

if [ -z "$repo" ] || [ -z "$head_sha" ]; then
  usage
  exit 2
fi

runs_json=$(gh api -H "Accept: application/vnd.github+json" \
  "repos/$repo/actions/runs?event=pull_request&head_sha=$head_sha&per_page=100")

ids=$(printf '%s' "$runs_json" | python3 -c '
import json
import sys

payload = json.load(sys.stdin)
runs = payload["workflow_runs"] if isinstance(payload, dict) else payload
head_sha = sys.argv[1]
for run in runs:
    if run.get("event") != "pull_request":
        continue
    if run.get("head_sha") != head_sha:
        continue
    if run.get("status") == "completed":
        continue
    print(run["id"])
' "$head_sha")

if [ -z "$ids" ]; then
  echo "cancel_pr_runs: nothing still running for $head_sha"
  exit 0
fi

failed=0
while IFS= read -r id; do
  [ -n "$id" ] || continue
  echo "cancel_pr_runs: cancelling run $id"
  if ! gh api -X POST "repos/$repo/actions/runs/$id/cancel"; then
    echo "cancel_pr_runs: could not cancel run $id" >&2
    failed=1
  fi
done <<EOF
$ids
EOF

exit "$failed"
