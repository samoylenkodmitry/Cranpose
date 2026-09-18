#!/usr/bin/env bash
set -euo pipefail

# A pull request's answer must arrive within twenty minutes. That is a policy,
# and a policy nothing checks is a wish, so this reads the workflows and fails
# if any job a pull request waits for may run longer than that.
#
# The cap is the enforcement too: a job that exceeds it fails rather than
# quietly making every branch wait. Work that cannot fit belongs on main or on
# the nightly schedule, which is where the whole robot suite and the
# architecture budgets run.

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

python3 - "$REPO_ROOT" <<'PY'
import sys, pathlib, yaml, re

BUDGET = 20
root = pathlib.Path(sys.argv[1])
failures = []
checked = 0

for path in sorted((root / ".github/workflows").glob("*.yml")):
    doc = yaml.safe_load(path.read_text())
    triggers = doc.get(True) or doc.get("on") or {}
    if "pull_request" not in (triggers if isinstance(triggers, dict) else {triggers: None}):
        continue
    for name, job in (doc.get("jobs") or {}).items():
        # Only a guard that is exactly this excludes pull requests. Matching
        # it as a substring would also swallow the fork guard
        # `github.event_name != 'pull_request' || <head is this repo>`, which
        # every mac job carries and which runs ON pull requests -- that read
        # skipped four of the eight jobs this exists to check. Anything this
        # cannot prove is PR-excluded is treated as PR-reachable.
        guard = " ".join(str(job.get("if", "")).split())
        if guard == "github.event_name != 'pull_request'":
            continue
        checked += 1
        timeout = job.get("timeout-minutes")
        if timeout is None:
            failures.append(f"{path.name}:{name} has no timeout-minutes")
            continue
        if isinstance(timeout, int):
            if timeout > BUDGET:
                failures.append(f"{path.name}:{name} allows {timeout} minutes")
            continue
        # An expression, which must pick the budget on pull requests.
        found = re.search(r"pull_request'\s*&&\s*(\d+)", str(timeout))
        if not found:
            failures.append(f"{path.name}:{name} has a timeout this check cannot read: {timeout}")
        elif int(found.group(1)) > BUDGET:
            failures.append(f"{path.name}:{name} allows {found.group(1)} minutes on a pull request")

if failures:
    print(f"FAIL: a pull request must be answered within {BUDGET} minutes:", file=sys.stderr)
    for line in failures:
        print(f"  {line}", file=sys.stderr)
    sys.exit(1)

print(f"pull-request budget: all {checked} job(s) capped at {BUDGET} minutes")
PY
