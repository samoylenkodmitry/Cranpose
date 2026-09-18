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

python3 - "$REPO_ROOT" <<'PYTHON_BUDGET'
import sys, pathlib, re

# The standard library only. PyYAML is present on some machines in this fleet
# and absent on others, and a gate that runs both in `precommit` and in CI
# must not depend on which one it landed on -- importing it passed locally and
# took main red. The structure read here is this repository's own: a job key
# at two spaces under `jobs:`, its attributes at four.
BUDGET = 20
JOB = re.compile(r"^  ([A-Za-z0-9_-]+):\s*$")
ATTR = re.compile(r"^    ([a-z-]+):\s*(.*)$")
ON_PULL_REQUEST = re.compile(r"^  pull_request:\s*$")

root = pathlib.Path(sys.argv[1])
failures = []
checked = 0

def judge(path, name, attrs):
    global checked
    # Only a guard that is exactly this excludes pull requests. Matching it as
    # a substring would also swallow the fork guard
    # `github.event_name != 'pull_request' || <head is this repo>`, which every
    # mac job carries and which runs ON pull requests.
    if " ".join(attrs.get("if", "").split()) == "github.event_name != 'pull_request'":
        return
    checked += 1
    timeout = attrs.get("timeout-minutes")
    if timeout is None:
        failures.append(f"{path.name}:{name} has no timeout-minutes")
        return
    if timeout.isdigit():
        if int(timeout) > BUDGET:
            failures.append(f"{path.name}:{name} allows {timeout} minutes")
        return
    found = re.search(r"pull_request'\s*&&\s*(\d+)", timeout)
    if not found:
        failures.append(f"{path.name}:{name} has a timeout this check cannot read: {timeout}")
    elif int(found.group(1)) > BUDGET:
        failures.append(f"{path.name}:{name} allows {found.group(1)} minutes on a pull request")

for path in sorted((root / ".github/workflows").glob("*.yml")):
    lines = path.read_text().split("\n")
    try:
        jobs_at = next(i for i, line in enumerate(lines) if line.rstrip() == "jobs:")
    except StopIteration:
        failures.append(f"{path.name} has no `jobs:` block; this check cannot read it")
        continue

    if not any(ON_PULL_REQUEST.match(line) for line in lines[:jobs_at]):
        continue

    parsed = 0
    name, attrs = None, {}
    for line in lines[jobs_at + 1:]:
        if line.strip() and not line.startswith(" "):
            break
        job = JOB.match(line)
        if job:
            if name is not None:
                parsed += 1
                judge(path, name, attrs)
            name, attrs = job.group(1), {}
            continue
        attr = ATTR.match(line)
        if attr and name is not None:
            attrs.setdefault(attr.group(1), attr.group(2).strip())
    if name is not None:
        parsed += 1
        judge(path, name, attrs)

    # A parser that reads nothing must say so rather than pass. An earlier
    # version of this check silently examined four of the eight jobs.
    if parsed == 0:
        failures.append(f"{path.name} triggers on pull_request but no job was parsed")

if checked == 0:
    failures.append("no pull-request job was checked at all")

if failures:
    print(f"FAIL: a pull request must be answered within {BUDGET} minutes:", file=sys.stderr)
    for line in failures:
        print(f"  {line}", file=sys.stderr)
    sys.exit(1)

print(f"pull-request budget: all {checked} job(s) capped at {BUDGET} minutes")
PYTHON_BUDGET
