#!/usr/bin/env python3
"""Copy-paste budget for clones a diff introduces -- to old code or to itself.

Scope is diff-only (see `gate_diff.py`): `jscpd` scans the whole repository,
because a new block can duplicate anything anywhere, but a clone only enters
consideration when at least one of its two copies falls inside the diff. Two
pre-existing clones that the diff does not touch are left alone, so the
workspace's existing duplication (measured at 1,592 clones over 10 lines as
of 2026-08-29) does not need to be cleaned up for this to pass.

Touching a clone is not the same as introducing it. A diff can land inside a
file jscpd already considers duplicated -- most commonly because a nearby
comment removal collapsed a block onto fewer lines, the same kind of forced,
comment-removal-triggered rewrite `complexity_gate.py` had to stop punishing
-- without the pre-existing duplication being that diff's fault or this
diff's job to pay down (see `docs/complexity_debt.md`). So this gate reruns
`jscpd` a second time against `base`'s pre-change content for exactly the
files a candidate clone involves, and only fails when no comparable
duplication already existed between the same two files: see
`already_duplicated_before`.

    just duplication-gate                  # CI's invocation
    python3 scripts/ci/duplication_gate.py --base origin/main
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


def load_duplication_config(config_path: Path) -> dict:
    with config_path.open("rb") as handle:
        config = tomllib.load(handle)
    return config["duplication"]


def run_jscpd(
    jscpd: Path,
    min_lines: int,
    min_tokens: int,
    ignore_globs: list[str],
    out_dir: Path,
    cwd: Path = ROOT,
) -> list[dict]:
    """Run `jscpd` against every file under `cwd` and return its `duplicates`.

    `cwd` defaults to the real checkout (`ROOT`) but is overridable to scan
    a disposable directory instead -- `already_duplicated_before` needs
    `jscpd`'s opinion of `base`'s pre-change content for the specific files
    a candidate clone involves, scoped by pointing `cwd` at a directory
    holding only those (`gate_diff.write_old_blobs`), not a second
    full-repo scan. Always scans `cwd` itself (`.`) rather than being handed
    an explicit file list: `jscpd` reports each match's file under `name`
    as a bare basename when given explicit paths, but as the full path
    relative to the scan root when given a directory to walk -- and a full
    relative path is what lets a result be matched back against `ranges`'s
    keys or another run's `name` values at all.
    """
    args = [
        str(jscpd),
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
    result = subprocess.run(args, cwd=cwd, capture_output=True, text=True)
    report_path = out_dir / "jscpd-report.json"
    if not report_path.exists():
        raise SystemExit(
            "duplication-gate: jscpd produced no report\n"
            f"stdout: {result.stdout}\nstderr: {result.stderr}"
        )
    return json.loads(report_path.read_text()).get("duplicates", [])


def side_span(side: dict) -> tuple[str, tuple[int, int]]:
    return side["name"], (side["start"], side["end"])


def file_pair(dup: dict) -> frozenset[str]:
    """The two files a duplicate spans, as an order-independent identity."""
    return frozenset({dup["firstFile"]["name"], dup["secondFile"]["name"]})


def touches_diff(dup: dict, ranges: dict[str, list[tuple[int, int]]]) -> bool:
    """Whether either side of `dup` falls inside `ranges`."""
    first_file, first_span = side_span(dup["firstFile"])
    second_file, second_span = side_span(dup["secondFile"])
    return gate_diff.any_intersect(
        ranges.get(first_file, []), first_span
    ) or gate_diff.any_intersect(ranges.get(second_file, []), second_span)


def already_duplicated_before(candidate: dict, old_duplicates: list[dict]) -> bool:
    """Whether some comparable duplication already existed between the same
    two files before this diff.

    Matched by file pair, not by exact line span or byte-identical content:
    the very cases this exists for are ones where the diff's nearby,
    comment-removal-forced rewrite (see the module docstring) shifted line
    numbers or reshaped the matched fragment's exact text without changing
    what code it is. Requiring an exact match would fail every one of those
    on a technicality and defeat the point.

    What is still required, so this does not degrade into "these two files
    have ever duplicated *anything*, so nothing between them ever counts
    again": the old duplicate's size (`lines`) must be within 2x of the
    candidate's, in either direction. Two files can plausibly hold more than
    one, unrelated clone pair; a same-file-pair match at wildly different
    size is more likely a different clone than the same one reformatted, and
    the conservative read -- treat it as unproven, not as license -- is the
    one that does not let a genuinely new clone hide behind an old,
    unrelated one.
    """
    candidate_lines = candidate.get("lines") or 0
    candidate_pair = file_pair(candidate)
    for old_dup in old_duplicates:
        if file_pair(old_dup) != candidate_pair:
            continue
        old_lines = old_dup.get("lines") or 0
        if old_lines == 0 or candidate_lines == 0:
            continue
        ratio = old_lines / candidate_lines
        if 0.5 <= ratio <= 2.0:
            return True
    return False


def find_violations(
    new_duplicates: list[dict],
    old_duplicates: list[dict],
    ranges: dict[str, list[tuple[int, int]]],
) -> list[str]:
    """Clones the diff introduces, not clones it happens to touch.

    A clone pair only enters consideration when at least one side falls in
    `ranges` (unchanged from before: a clone neither side of which the diff
    touched at all is out of scope regardless of history). Among those, one
    already comparably duplicated before this diff (see
    `already_duplicated_before`) is pre-existing debt -- recorded in
    `docs/complexity_debt.md`, not this diff's to pay down -- and passes;
    one with nothing comparable on the old side is new, and fails.
    """
    old_by_pair: dict[frozenset[str], list[dict]] = {}
    for old_dup in old_duplicates:
        old_by_pair.setdefault(file_pair(old_dup), []).append(old_dup)

    violations: list[str] = []
    for dup in new_duplicates:
        if not touches_diff(dup, ranges):
            continue
        if already_duplicated_before(dup, old_by_pair.get(file_pair(dup), [])):
            continue
        first_file, first_span = side_span(dup["firstFile"])
        second_file, second_span = side_span(dup["secondFile"])
        first_hit = gate_diff.any_intersect(ranges.get(first_file, []), first_span)
        second_hit = gate_diff.any_intersect(ranges.get(second_file, []), second_span)
        lines = dup.get("lines")
        violations.append(
            f"{first_file}:{first_span[0]}-{first_span[1]}"
            f"{' (new)' if first_hit else ''}"
            " duplicates "
            f"{second_file}:{second_span[0]}-{second_span[1]}"
            f"{' (new)' if second_hit else ''}"
            f" ({lines} lines, introduced by this diff)"
        )
    return violations


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", default="origin/main")
    parser.add_argument("--config", type=Path, default=DEFAULT_CONFIG)
    args = parser.parse_args()

    config = load_duplication_config(args.config)
    ranges = gate_diff.changed_ranges(args.base, "*.rs")
    if not ranges:
        print(f"duplication-gate: no changed Rust files against {args.base}")
        return 0

    jscpd = gate_diff.resolve_cargo_tool(
        "jscpd",
        "jscpd also ships an npm package that wraps a prebuilt copy of the same "
        "Rust binary, but this project does not require Node.js and will not "
        "start requiring it here -- if `cargo install jscpd --locked` cannot "
        "run, fix that (network, registry, offline mirror), do not swap in "
        "`npm install -g jscpd`.",
    )
    base_sha = gate_diff.merge_base(args.base)
    with tempfile.TemporaryDirectory() as tmp:
        tmp_path = Path(tmp)
        new_out = tmp_path / "new-out"
        new_out.mkdir()
        duplicates = run_jscpd(
            jscpd,
            config["min_lines"],
            config["min_tokens"],
            config.get("ignore_globs", []),
            new_out,
        )

        candidates = [dup for dup in duplicates if touches_diff(dup, ranges)]
        involved_files = sorted(
            {dup["firstFile"]["name"] for dup in candidates}
            | {dup["secondFile"]["name"] for dup in candidates}
        )
        old_duplicates: list[dict] = []
        if involved_files:
            old_src = tmp_path / "old-src"
            old_src.mkdir()
            old_files = gate_diff.write_old_blobs(base_sha, involved_files, old_src)
            if old_files:
                old_out = tmp_path / "old-out"
                old_out.mkdir()
                old_duplicates = run_jscpd(
                    jscpd,
                    config["min_lines"],
                    config["min_tokens"],
                    config.get("ignore_globs", []),
                    old_out,
                    cwd=old_src,
                )

    violations = find_violations(duplicates, old_duplicates, ranges)
    if violations:
        print(
            f"duplication-gate: {len(violations)} clone(s) introduced by this diff:",
            file=sys.stderr,
        )
        for v in violations:
            print(f"  {v}", file=sys.stderr)
        return 1

    print(
        f"duplication-gate: {len(duplicates)} clone(s) in the repo, "
        f"{len(candidates)} touch the diff, none introduced by it"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
