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


def functions_in(space: dict, out: list[dict], inside_function: bool = False) -> None:
    """Flatten `rust-code-analysis-cli`'s nested space tree into functions.

    Closures are their own `kind == "function"` spaces nested inside the
    function that defines them -- `space.get("name")` is unset for exactly
    these, never for a named `fn` item, however deeply nested. A closure
    already inside another function is not independently-invocable code, it
    *is* that function's own body, just packaged as a value; reporting both
    counted the same bytes twice; every function this diff has ever flagged
    for having "a real function and an oversized closure nested in it" was
    one problem being reported as two. Only the outermost boundary of such a
    chain is reportable -- `inside_function` tracks whether the recursion
    has already passed one -- so a closure nested in a closure still
    collapses to just the one enclosing report, while a named `fn` defined
    inside a closure's body (legal, if unusual, Rust) is still independently
    real code and is still reported no matter how deep the nesting.
    """
    is_function = space.get("kind") == "function"
    raw_name = space.get("name")
    # `rust-code-analysis-cli` itself already emits the literal string
    # "<anonymous>" for a closure's own `name` field -- it is not left
    # `None`/empty, so a plain truthiness check treats every closure as
    # "named" and never collapses anything. Only a name that is neither
    # missing nor that exact sentinel is a real, independently-invocable
    # `fn` item.
    is_named = bool(raw_name) and raw_name != "<anonymous>"
    if is_function and (is_named or not inside_function):
        metrics = space.get("metrics") or {}
        cyclomatic = (metrics.get("cyclomatic") or {}).get("sum")
        out.append(
            {
                "name": raw_name if is_named else "<anonymous>",
                "start": space.get("start_line"),
                "end": space.get("end_line"),
                "cyclomatic": cyclomatic,
            }
        )
    for child in space.get("spaces", []):
        functions_in(child, out, inside_function=inside_function or is_function)


def analyze_files(
    rust_code_analysis_cli: Path, files: list[str], out_dir: Path, cwd: Path = ROOT
) -> dict[str, list[dict]]:
    """Run `rust-code-analysis-cli` on `files`, returning functions per file.

    The CLI takes one `-p <path>` per input (a comma-joined list is treated
    as one literal, nonexistent path, not a list) and mirrors each input's
    relative path under `out_dir` with a `.json` suffix appended. `cwd`
    defaults to the real checkout (`ROOT`) but is overridable so this same
    function can analyze a disposable directory of pre-change blobs
    (`gate_diff.write_old_blobs`) for the "before" half of a before/after
    comparison, keyed by the same relative paths either way.
    """
    args = [str(rust_code_analysis_cli), "-m", "-O", "json", "-o", str(out_dir)]
    for f in files:
        args += ["-p", f]
    result = subprocess.run(args, cwd=cwd, capture_output=True, text=True)
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


def old_complexity_by_name(functions: list[dict]) -> dict[str, list[int]]:
    """Named functions' cyclomatic complexity, in file order, keyed by name.

    A list rather than a single value per name: two unrelated methods can
    share a bare name in the same file (`fn new()` on two different
    structs), and matching the Nth occurrence of a name in the old file
    against the Nth occurrence in the new file is a real, if imperfect,
    improvement over "last one wins" -- it only misjudges a function whose
    diff also reordered it past another same-named function, which is both
    rarer and lower-stakes than silently comparing two unrelated functions
    because they happened to share a name. Anonymous entries (`<anonymous>`,
    meaning a closure with no enclosing function to fold into -- see
    `functions_in`) have no stable identity across a diff and are excluded;
    a function that cannot be matched has no "before" to compare against.
    """
    by_name: dict[str, list[int]] = {}
    for func in functions:
        if func["name"] == "<anonymous>" or func["cyclomatic"] is None:
            continue
        by_name.setdefault(func["name"], []).append(int(func["cyclomatic"]))
    return by_name


def find_violations(
    ranges: dict[str, list[tuple[int, int]]],
    new_functions_by_file: dict[str, list[dict]],
    old_functions_by_file: dict[str, list[dict]],
    max_cyclomatic: int,
) -> list[str]:
    """Functions the diff made worse, not functions it merely touches.

    A function already over `max_cyclomatic` before this diff is existing
    debt (see `docs/complexity_debt.md`), not something this diff is asked
    to fix as the price of touching a nearby line. The gate instead compares
    each touched function's complexity before and after: unchanged or lower
    passes regardless of the absolute number, higher fails regardless of it
    -- a diff that pushes a function from 18 to 25 crossed the limit itself
    and fails exactly like one that pushed 174 to 175, while one that
    deleted a comment inside a 174-complexity function and left its control
    flow untouched passes, because it did not make anything worse. A
    function with no old counterpart (new code, or a name this pass could
    not match -- see `old_complexity_by_name`) has nothing to be "not worse
    than," so it is judged against the limit directly, same as before.
    """
    violations: list[str] = []
    for file, functions in sorted(new_functions_by_file.items()):
        file_ranges = ranges.get(file, [])
        old_by_name = old_complexity_by_name(old_functions_by_file.get(file, []))
        cursor: dict[str, int] = {}
        for func in functions:
            occurrence = cursor.get(func["name"], 0)
            cursor[func["name"]] = occurrence + 1
            if func["start"] is None or func["end"] is None or func["cyclomatic"] is None:
                continue
            span = (func["start"], func["end"])
            if not gate_diff.any_intersect(file_ranges, span):
                continue
            new_cyclomatic = int(func["cyclomatic"])
            if new_cyclomatic <= max_cyclomatic:
                continue
            old_values = old_by_name.get(func["name"], [])
            old_cyclomatic = old_values[occurrence] if occurrence < len(old_values) else None
            if old_cyclomatic is not None and new_cyclomatic <= old_cyclomatic:
                continue
            reason = (
                f"was {old_cyclomatic}, is now {new_cyclomatic}"
                if old_cyclomatic is not None
                else f"is new at {new_cyclomatic}"
            )
            violations.append(
                f"{file}:{span[0]}-{span[1]} {func['name']} {reason} (limit {max_cyclomatic})"
            )
    return violations


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", default="origin/main")
    parser.add_argument("--config", type=Path, default=DEFAULT_CONFIG)
    args = parser.parse_args()

    max_cyclomatic = load_max_cyclomatic(args.config)
    ranges = gate_diff.changed_ranges(args.base, "*.rs")
    if not ranges:
        print(f"complexity-gate: no changed Rust files against {args.base}")
        return 0

    rust_code_analysis_cli = gate_diff.resolve_cargo_tool("rust-code-analysis-cli")
    base_sha = gate_diff.merge_base(args.base)
    with tempfile.TemporaryDirectory() as tmp:
        tmp_path = Path(tmp)
        new_out = tmp_path / "new-out"
        new_out.mkdir()
        new_functions_by_file = analyze_files(rust_code_analysis_cli, sorted(ranges), new_out)

        old_src = tmp_path / "old-src"
        old_src.mkdir()
        old_files = gate_diff.write_old_blobs(base_sha, sorted(ranges), old_src)
        old_functions_by_file = {}
        if old_files:
            old_out = tmp_path / "old-out"
            old_out.mkdir()
            old_functions_by_file = analyze_files(
                rust_code_analysis_cli, old_files, old_out, cwd=old_src
            )

    violations = find_violations(
        ranges, new_functions_by_file, old_functions_by_file, max_cyclomatic
    )
    if violations:
        print(f"complexity-gate: {len(violations)} function(s) over the limit:", file=sys.stderr)
        for v in violations:
            print(f"  {v}", file=sys.stderr)
        return 1

    print(f"complexity-gate: {sum(len(v) for v in ranges.values())} changed line range(s) clean")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
