#!/usr/bin/env python3
"""Cyclomatic complexity ceiling for functions a diff adds or modifies.

Scope is diff-only (see `gate_diff.py`): a function already over the limit is
left alone unless the diff touches it, and the workspace does not need to
pass this retroactively. `rust-code-analysis-cli` parses the changed files
without compiling them, so this only ever costs milliseconds against however
many files the diff touched, never a workspace build.

    just complexity-gate                  # CI's invocation
    python3 scripts/ci/complexity_gate.py --base origin/main
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


def load_max_cyclomatic(config_path: Path) -> int:
    with config_path.open("rb") as handle:
        config = tomllib.load(handle)
    return int(config["complexity"]["max_cyclomatic"])


def functions_in(space: dict, out: list[dict]) -> None:
    """Flatten `rust-code-analysis-cli`'s nested space tree into functions.

    Closures are their own `kind == "function"` spaces nested inside the
    function that defines them, so this recurses into every space -- a
    closure the diff adds should be checked on its own span, not only as
    part of whatever complexity its enclosing function reports.
    """
    if space.get("kind") == "function":
        metrics = space.get("metrics") or {}
        cyclomatic = (metrics.get("cyclomatic") or {}).get("sum")
        out.append(
            {
                "name": space.get("name") or "<anonymous>",
                "start": space.get("start_line"),
                "end": space.get("end_line"),
                "cyclomatic": cyclomatic,
            }
        )
    for child in space.get("spaces", []):
        functions_in(child, out)


def analyze_files(files: list[str], out_dir: Path) -> dict[str, list[dict]]:
    """Run `rust-code-analysis-cli` on `files`, returning functions per file.

    The CLI takes one `-p <path>` per input (a comma-joined list is treated
    as one literal, nonexistent path, not a list) and mirrors each input's
    relative path under `out_dir` with a `.json` suffix appended.
    """
    args = ["rust-code-analysis-cli", "-m", "-O", "json", "-o", str(out_dir)]
    for f in files:
        args += ["-p", f]
    result = subprocess.run(args, cwd=ROOT, capture_output=True, text=True)
    produced = list(out_dir.rglob("*.json"))
    if result.returncode != 0 and not produced:
        raise SystemExit(
            "complexity-gate: rust-code-analysis-cli failed with no output\n"
            f"stdout: {result.stdout}\nstderr: {result.stderr}"
        )
    if result.stderr.strip():
        print(result.stderr.strip(), file=sys.stderr)

    by_file: dict[str, list[dict]] = {}
    for f in files:
        json_path = out_dir / f"{f}.json"
        if not json_path.exists():
            print(
                f"complexity-gate: no analysis produced for {f}, skipping",
                file=sys.stderr,
            )
            continue
        try:
            data = json.loads(json_path.read_text())
        except json.JSONDecodeError as exc:
            print(f"complexity-gate: could not parse analysis for {f}: {exc}", file=sys.stderr)
            continue
        functions: list[dict] = []
        functions_in(data, functions)
        by_file[f] = functions
    return by_file


def find_violations(
    ranges: dict[str, list[tuple[int, int]]],
    functions_by_file: dict[str, list[dict]],
    max_cyclomatic: int,
) -> list[str]:
    violations: list[str] = []
    for file, functions in sorted(functions_by_file.items()):
        file_ranges = ranges.get(file, [])
        for func in functions:
            if func["start"] is None or func["end"] is None or func["cyclomatic"] is None:
                continue
            span = (func["start"], func["end"])
            if not gate_diff.any_intersect(file_ranges, span):
                continue
            cyclomatic = int(func["cyclomatic"])
            if cyclomatic > max_cyclomatic:
                violations.append(
                    f"{file}:{span[0]}-{span[1]} {func['name']} "
                    f"has cyclomatic complexity {cyclomatic} (limit {max_cyclomatic})"
                )
    return violations


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", default="origin/main")
    parser.add_argument("--config", type=Path, default=DEFAULT_CONFIG)
    args = parser.parse_args()

    if shutil.which("rust-code-analysis-cli") is None:
        print(
            "complexity-gate: rust-code-analysis-cli not found; "
            "install with `cargo install rust-code-analysis-cli --locked`",
            file=sys.stderr,
        )
        return 1

    max_cyclomatic = load_max_cyclomatic(args.config)
    ranges = gate_diff.changed_ranges(args.base, "*.rs")
    if not ranges:
        print(f"complexity-gate: no changed Rust files against {args.base}")
        return 0

    with tempfile.TemporaryDirectory() as tmp:
        functions_by_file = analyze_files(sorted(ranges), Path(tmp))

    violations = find_violations(ranges, functions_by_file, max_cyclomatic)
    if violations:
        print(f"complexity-gate: {len(violations)} function(s) over the limit:", file=sys.stderr)
        for v in violations:
            print(f"  {v}", file=sys.stderr)
        return 1

    print(f"complexity-gate: {sum(len(v) for v in ranges.values())} changed line range(s) clean")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
