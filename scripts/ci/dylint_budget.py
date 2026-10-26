#!/usr/bin/env python3
"""The workspace's dylint lints (lints/), with a budget for each lint's sites.

    dylint_budget.py            # fail when a lint's count differs from its budget
    dylint_budget.py --lower    # also write the counts that fell below their budget

Runs `cargo dylint --all` over the workspace's targets, counts each lint's
sites once (a site linted in the library and again in its test target is one
site), and compares the counts with scripts/ci/dylint_budget.toml. A higher
count names the new sites; a lower one asks for the budget to come down in the
same commit, so the saving stays.
"""

import json
import os
import pathlib
import subprocess
import sys
import tomllib

ROOT = pathlib.Path(__file__).resolve().parents[2]
BUDGET_FILE = ROOT / "scripts/ci/dylint_budget.toml"
TARGET_DIR = ROOT / "target/dylint-workspace"


def load_budgets() -> dict[str, int]:
    return tomllib.loads(BUDGET_FILE.read_text())["lints"]


def write_budgets(budgets: dict[str, int]) -> None:
    text = BUDGET_FILE.read_text()
    head = text[: text.index("[lints]")]
    body = "".join(f"{lint} = {count}\n" for lint, count in sorted(budgets.items()))
    BUDGET_FILE.write_text(f"{head}[lints]\n{body}")


def lint_sites() -> dict[str, set[tuple[str, int, int]]]:
    command = [
        "cargo", "dylint", "--all", "--",
        "--workspace", "--all-targets", "--keep-going", "--message-format=json",
    ]
    env = {**os.environ, "CARGO_TARGET_DIR": str(TARGET_DIR)}
    result = subprocess.run(command, cwd=ROOT, capture_output=True, text=True, env=env)
    sites: dict[str, set[tuple[str, int, int]]] = {}
    errors = []
    for line in result.stdout.splitlines():
        try:
            record = json.loads(line)
        except json.JSONDecodeError:
            continue
        if record.get("reason") != "compiler-message":
            continue
        message = record["message"]
        if message["level"] == "error":
            errors.append(message.get("rendered") or message["message"])
            continue
        code = (message.get("code") or {}).get("code")
        primary = next((span for span in message["spans"] if span["is_primary"]), None)
        if code and primary:
            site = (primary["file_name"], primary["line_start"], primary["column_start"])
            sites.setdefault(code, set()).add(site)
    if result.returncode != 0:
        sys.stderr.write("".join(errors) or result.stderr)
        raise SystemExit(f"dylint-budget: cargo dylint exited {result.returncode}")
    return sites


def main(argv: list[str]) -> int:
    lower = argv[1:] == ["--lower"]
    budgets = load_budgets()
    sites = lint_sites()
    failures = []
    lowered = False
    for lint, budget in budgets.items():
        found = sites.get(lint, set())
        if len(found) > budget:
            failures.append(f"{lint}: {len(found)} sites, over the budget of {budget}")
            for file, line, column in sorted(found):
                print(f"  {file}:{line}:{column}", file=sys.stderr)
        elif len(found) < budget:
            if lower:
                budgets[lint] = len(found)
                lowered = True
            else:
                failures.append(f"{lint}: {len(found)} sites, under the budget of {budget}")
    for lint in sites.keys() - budgets.keys():
        failures.append(f"{lint}: {len(sites[lint])} sites and no budget")
    if lowered:
        write_budgets(budgets)
        print(f"dylint-budget: lowered budgets in {BUDGET_FILE.relative_to(ROOT)}")
    if failures:
        for failure in failures:
            print(f"dylint-budget: {failure}", file=sys.stderr)
        if not lower:
            print("dylint-budget: `just dylint-lower` writes the counts that fell", file=sys.stderr)
        return 1
    print("dylint-budget: within budget (" + ", ".join(f"{k} {v}" for k, v in sorted(budgets.items())) + ")")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
