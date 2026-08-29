#!/usr/bin/env python3
"""Which line ranges a change touches, for the diff-scoped quality gates.

`complexity_gate.py` and `duplication_gate.py` both need the same answer to
the same question: which lines, in which files, did this change add or
modify? Neither gate is allowed to fail on code the diff never touched, so
both derive their scope from here rather than from a repo-wide scan.

"The diff" is `git diff --unified=0 --no-prefix <merge-base>`, with no second
ref: that compares the merge base against the working tree, which also covers
staged and unstaged edits a developer has not committed yet. In CI the working
tree is exactly the checked-out commit, so the same command means the same
thing in both places.

The merge base is computed against the ref given (default `origin/main`), not
hardcoded to a branch name or a commit, so this works for any PR and for a
plain feature branch alike. A shallow checkout (`actions/checkout`'s default)
may not have the commits needed to compute it; `ensure_ref` fetches the base
branch when it is not resolvable rather than failing with a confusing
`git merge-base` error.
"""

from __future__ import annotations

import re
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]

_HUNK_HEADER = re.compile(r"^@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@")


def ensure_ref(base: str) -> None:
    """Make `base` resolvable and current.

    A persistent self-hosted runner's checkout is not fresh by default: a
    prior job can leave `origin/main` resolvable but pointing at a stale
    commit, which computes a stale merge base and silently mis-scopes the
    diff rather than failing loudly. So this always re-fetches `base` when it
    looks like `origin/<branch>`, rather than only fetching when the ref is
    entirely missing (the shallow-checkout case). A fetch failure is only
    fatal if `base` does not already resolve to *something* locally -- a
    developer running this offline with a same-day `origin/main` should not
    be blocked by the network.
    """
    if base.startswith("origin/"):
        branch = base.removeprefix("origin/")
        subprocess.run(["git", "fetch", "--quiet", "origin", branch], cwd=ROOT)
    resolved = subprocess.run(
        ["git", "rev-parse", "--verify", "-q", base],
        cwd=ROOT,
        capture_output=True,
    )
    if resolved.returncode != 0:
        raise SystemExit(
            f"gate_diff: cannot resolve base ref {base!r}: it fetched nothing "
            "and no local ref by that name exists"
        )


def merge_base(base: str) -> str:
    """The merge base of `base` and HEAD, fetching `base` first if needed."""
    ensure_ref(base)
    result = subprocess.run(
        ["git", "merge-base", base, "HEAD"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=True,
    )
    return result.stdout.strip()


def parse_unified_diff_zero_context(diff_text: str) -> dict[str, list[tuple[int, int]]]:
    """Parse `git diff --unified=0 --no-prefix` output into changed ranges.

    Returns one inclusive `(start_line, end_line)` range per hunk, in the
    *new* (post-change) file's line numbering, keyed by that file's path.
    A hunk that only deletes lines (`+l,0`) contributes no range: there is no
    added or modified line on the new side to hold accountable. A deleted
    file (`+++ /dev/null`) is skipped for the same reason -- nothing in it can
    be checked against, because nothing in it still exists.
    """
    ranges: dict[str, list[tuple[int, int]]] = {}
    current_file: str | None = None
    for line in diff_text.splitlines():
        if line.startswith("+++ "):
            path = line[len("+++ ") :]
            current_file = None if path == "/dev/null" else path
            continue
        if not line.startswith("@@ "):
            continue
        if current_file is None:
            continue
        match = _HUNK_HEADER.match(line)
        if not match:
            continue
        start = int(match.group(1))
        count = int(match.group(2)) if match.group(2) is not None else 1
        if count == 0:
            continue
        ranges.setdefault(current_file, []).append((start, start + count - 1))
    return ranges


def changed_ranges(base: str, pathspec: str = "*.rs") -> dict[str, list[tuple[int, int]]]:
    """Changed line ranges per file, restricted to `pathspec`, vs `base`."""
    base_sha = merge_base(base)
    result = subprocess.run(
        ["git", "diff", "--unified=0", "--no-prefix", base_sha, "--", pathspec],
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=True,
    )
    return parse_unified_diff_zero_context(result.stdout)


def intersects(a: tuple[int, int], b: tuple[int, int]) -> bool:
    """Whether two inclusive `(start, end)` line ranges overlap."""
    return a[0] <= b[1] and b[0] <= a[1]


def any_intersect(ranges: list[tuple[int, int]], span: tuple[int, int]) -> bool:
    return any(intersects(r, span) for r in ranges)
