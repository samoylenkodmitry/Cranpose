#!/usr/bin/env python3
"""Workspace clippy with a budget for each lint that still has hits.

    clippy_budget.py            # fail on any warning outside the budget, or a count off budget
    clippy_budget.py --lower    # also write the counts that fell below their budget
    clippy_budget.py --count    # print every budgeted lint's count and change nothing

A lint with no hits left belongs in `[workspace.lints]` of Cargo.toml, where
every clippy run of every target denies it. A lint that still has hits is
listed in clippy_budget.toml with today's count: this script turns it on as a
warning for one workspace run, counts each site once (a site linted in the
library and again in its test target is one site) and fails when the count
differs from the budget. A higher count names the sites in files the branch
changed; a lower one asks for the budget to come down in the same commit, so
the saving stays. Any other warning fails as `-D warnings` would.
"""

import json
import pathlib
import subprocess
import sys
import tomllib

ROOT = pathlib.Path(__file__).resolve().parents[2]
BUDGET_FILE = ROOT / "scripts/ci/clippy_budget.toml"


def load_budgets() -> dict[str, int]:
    return tomllib.loads(BUDGET_FILE.read_text())["lints"]


def write_budgets(budgets: dict[str, int]) -> None:
    text = BUDGET_FILE.read_text()
    head = text[: text.index("[lints]")]
    body = "".join(f'"{lint}" = {count}\n' for lint, count in sorted(budgets.items()))
    BUDGET_FILE.write_text(f"{head}[lints]\n{body}")


def lint_flag(lint: str) -> list[str]:
    return ["-W", lint]


def run_clippy(lints: list[str]) -> list[dict]:
    command = ["cargo", "clippy", "--workspace", "--all-targets", "--keep-going", "--message-format=json", "--"]
    for lint in lints:
        command += lint_flag(lint)
    result = subprocess.run(command, cwd=ROOT, capture_output=True, text=True)
    messages = []
    for line in result.stdout.splitlines():
        try:
            record = json.loads(line)
        except json.JSONDecodeError:
            continue
        if record.get("reason") == "compiler-message":
            messages.append(record["message"])
    if result.returncode != 0 and not any(m["level"] == "error" for m in messages):
        sys.stderr.write(result.stderr)
        raise SystemExit(f"clippy-budget: cargo clippy exited {result.returncode}")
    return messages


def site(message: dict) -> tuple[str, int, int] | None:
    for span in message["spans"]:
        if span["is_primary"]:
            return span["file_name"], span["line_start"], span["column_start"]
    return None


def changed_files() -> set[str]:
    base = subprocess.run(
        ["git", "merge-base", "HEAD", "origin/main"], cwd=ROOT, capture_output=True, text=True
    ).stdout.strip()
    if not base:
        return set()
    diff = subprocess.run(
        ["git", "diff", "--name-only", base], cwd=ROOT, capture_output=True, text=True
    ).stdout
    return set(diff.split())


def main(argv: list[str]) -> int:
    mode = argv[1] if len(argv) > 1 else ""
    if mode not in ("", "--lower", "--count"):
        print(__doc__, file=sys.stderr)
        return 2
    budgets = load_budgets()
    sites: dict[str, set[tuple[str, int, int]]] = {lint: set() for lint in budgets}
    unbudgeted: dict[tuple, str] = {}
    for message in run_clippy(list(budgets)):
        code = (message.get("code") or {}).get("code") or ""
        where = site(message)
        if message["level"] == "warning" and where is None:
            continue
        if code in sites and message["level"] == "warning":
            sites[code].add(where)
        elif message["level"] in ("warning", "error"):
            unbudgeted.setdefault((code, where, message["message"]), message["rendered"])

    counts = {lint: len(found) for lint, found in sites.items()}
    if mode == "--count":
        listing = ROOT / "target/clippy-budget-sites.json"
        listing.parent.mkdir(exist_ok=True)
        listing.write_text(json.dumps({lint: sorted(found) for lint, found in sites.items()}))
        for rendered in unbudgeted.values():
            sys.stdout.write(rendered)
        for lint, count in sorted(counts.items(), key=lambda item: -item[1]):
            print(f"{count:6} {lint} (budget {budgets[lint]})")
        print(f"{len(unbudgeted):6} other warnings or errors; sites in {listing.relative_to(ROOT)}")
        return 0

    failures = []
    for rendered in unbudgeted.values():
        sys.stdout.write(rendered)
    if unbudgeted:
        failures.append(f"{len(unbudgeted)} warning(s) or error(s) outside the budget")

    changed = changed_files()
    lowered = dict(budgets)
    for lint, count in sorted(counts.items()):
        budget = budgets[lint]
        if count > budget:
            new = sorted(s for s in sites[lint] if s[0] in changed)
            listing = "".join(f"\n    {file}:{line}:{column}" for file, line, column in new)
            failures.append(
                f"{lint}: {count} sites, over the budget of {budget}; fix the new ones"
                + (f" (in files this branch changed:{listing})" if listing else "")
            )
        elif count < budget:
            lowered[lint] = count
            if mode != "--lower":
                failures.append(
                    f"{lint}: {count} sites, under the budget of {budget}; run "
                    "`just clippy-budget-lower` so the saving stays"
                )
    if mode == "--lower" and lowered != budgets:
        write_budgets(lowered)
        print(f"clippy-budget: lowered budgets in {BUDGET_FILE.relative_to(ROOT)}")

    for failure in failures:
        print(f"clippy-budget: {failure}", file=sys.stderr)
    if not failures:
        total = sum(counts.values())
        print(f"clippy-budget: {len(counts)} budgeted lints, {total} sites, all within budget")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
