#!/usr/bin/env python3
"""Copy-paste budget for blocks a diff adds -- to old code or to itself.

Scope is diff-only (see `gate_diff.py`): `jscpd` scans the whole repository,
because a new block can duplicate anything anywhere, but a clone only fails
the gate when at least one of its two copies falls inside the diff. Two
pre-existing clones that the diff does not touch are left alone, so the
workspace's existing duplication (measured at 1,592 clones over 10 lines as
of 2026-08-29) does not need to be cleaned up for this to pass.

    just duplication-gate                  # CI's invocation
    python3 scripts/ci/duplication_gate.py --base origin/main
"""

from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path

import gate_diff

ROOT = Path(__file__).resolve().parents[2]
DEFAULT_CONFIG = Path(__file__).resolve().parent / "code_quality_gates.toml"


def load_duplication_config(config_path: Path) -> dict:
    with config_path.open("rb") as handle:
        config = tomllib.load(handle)
    return config["duplication"]


def run_jscpd(min_lines: int, min_tokens: int, ignore_globs: list[str], out_dir: Path) -> list[dict]:
    args = [
        "jscpd",
        "--min-lines",
        str(min_lines),
        "--min-tokens",
        str(min_tokens),
        "-f",
        "rust",
        "-r",
        "json",
        "-o",
        str(out_dir),
    ]
    if ignore_globs:
        args += ["--ignore", ",".join(ignore_globs)]
    args.append(".")
    result = subprocess.run(args, cwd=ROOT, capture_output=True, text=True)
    report_path = out_dir / "jscpd-report.json"
    if not report_path.exists():
        raise SystemExit(
            "duplication-gate: jscpd produced no report\n"
            f"stdout: {result.stdout}\nstderr: {result.stderr}"
        )
    return json.loads(report_path.read_text()).get("duplicates", [])


def side_span(side: dict) -> tuple[str, tuple[int, int]]:
    return side["name"], (side["start"], side["end"])


def find_violations(
    duplicates: list[dict], ranges: dict[str, list[tuple[int, int]]]
) -> list[str]:
    violations: list[str] = []
    for dup in duplicates:
        first_file, first_span = side_span(dup["firstFile"])
        second_file, second_span = side_span(dup["secondFile"])
        first_hit = gate_diff.any_intersect(ranges.get(first_file, []), first_span)
        second_hit = gate_diff.any_intersect(ranges.get(second_file, []), second_span)
        if not (first_hit or second_hit):
            continue
        lines = dup.get("lines")
        violations.append(
            f"{first_file}:{first_span[0]}-{first_span[1]}"
            f"{' (new)' if first_hit else ''}"
            " duplicates "
            f"{second_file}:{second_span[0]}-{second_span[1]}"
            f"{' (new)' if second_hit else ''}"
            f" ({lines} lines)"
        )
    return violations


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", default="origin/main")
    parser.add_argument("--config", type=Path, default=DEFAULT_CONFIG)
    args = parser.parse_args()

    if shutil.which("jscpd") is None:
        print(
            "duplication-gate: jscpd not found; install with `cargo install jscpd --locked`",
            file=sys.stderr,
        )
        return 1

    config = load_duplication_config(args.config)
    ranges = gate_diff.changed_ranges(args.base, "*.rs")
    if not ranges:
        print(f"duplication-gate: no changed Rust files against {args.base}")
        return 0

    with tempfile.TemporaryDirectory() as tmp:
        duplicates = run_jscpd(
            config["min_lines"], config["min_tokens"], config.get("ignore_globs", []), Path(tmp)
        )

    violations = find_violations(duplicates, ranges)
    if violations:
        print(f"duplication-gate: {len(violations)} clone(s) touch the diff:", file=sys.stderr)
        for v in violations:
            print(f"  {v}", file=sys.stderr)
        return 1

    print(f"duplication-gate: {len(duplicates)} clone(s) in the repo, none touch the diff")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
