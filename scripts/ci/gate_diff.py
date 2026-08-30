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
leaves `base` and `HEAD` as two problems, not one: `ensure_ref` makes `base`
resolvable by fetching it, and `merge_base` separately makes it *reachable*
from `HEAD` by deepening history when the two sides turn out to be
disconnected single-commit graphs with no merge base between them.
"""

from __future__ import annotations

import os
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


def is_shallow_repository() -> bool:
    """Whether the checkout's history is truncated at some boundary commit."""
    result = subprocess.run(
        ["git", "rev-parse", "--is-shallow-repository"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=True,
    )
    return result.stdout.strip() == "true"


def merge_base(base: str) -> str:
    """The merge base of `base` and HEAD, deepening history first if needed.

    `actions/checkout`'s default depth-1 clone gives `HEAD` a history of
    exactly one commit with no recorded parent; `ensure_ref`'s own fetch of
    `base` is likewise shallow. The two land as disconnected single-commit
    graphs, so `git merge-base` exits 1 even though `base` resolves fine --
    not because the branches lack a common ancestor, but because neither
    side's local history reaches back far enough to see it. `git fetch
    --unshallow` is the fix, tried only after a first attempt fails and only
    when the repository is actually shallow: a non-shallow repository
    failing the same merge-base call really does share no history with
    `base`, and this refuses to paper over that by guessing a diff scope --
    the exact silent mis-scoping the diff-scoped gates exist to prevent.
    """
    ensure_ref(base)
    first_attempt = subprocess.run(
        ["git", "merge-base", base, "HEAD"],
        cwd=ROOT,
        capture_output=True,
        text=True,
    )
    if first_attempt.returncode == 0:
        return first_attempt.stdout.strip()
    if not is_shallow_repository():
        raise SystemExit(
            f"gate_diff: no merge base between {base!r} and HEAD, and the "
            "checkout already has full history -- these two branches share "
            "no common ancestor, so the diff cannot be scoped"
        )
    deepened = subprocess.run(["git", "fetch", "--quiet", "--unshallow", "origin"], cwd=ROOT)
    if deepened.returncode != 0:
        raise SystemExit(
            "gate_diff: the checkout is shallow and `git fetch --unshallow "
            "origin` failed -- cannot deepen history enough to find a "
            f"merge base with {base!r}"
        )
    second_attempt = subprocess.run(
        ["git", "merge-base", base, "HEAD"],
        cwd=ROOT,
        capture_output=True,
        text=True,
    )
    if second_attempt.returncode != 0:
        raise SystemExit(
            f"gate_diff: no merge base between {base!r} and HEAD even after "
            "`git fetch --unshallow` -- cannot scope the diff safely"
        )
    return second_attempt.stdout.strip()


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


def cargo_bin_dir() -> Path:
    """Where `cargo install` puts binaries -- $CARGO_HOME/bin, or ~/.cargo/bin.

    Every gate tool is resolved against this exact path, never against
    `$PATH`. `$PATH` is not deterministic across machines: a name like
    `jscpd` can also be a leftover npm global, an old major version, or a
    system package, and a bare `shutil.which()` / `command -v` probe accepts
    whichever one happens to be sitting there first. Pinning the path pins
    the binary to the one this project just installed.
    """
    cargo_home = os.environ.get("CARGO_HOME")
    root = Path(cargo_home).expanduser() if cargo_home else Path.home() / ".cargo"
    return root / "bin"


def resolve_cargo_tool(name: str, on_install_failure_hint: str = "") -> Path:
    """The cargo-installed `name`, installing it first if it is missing.

    Never falls back to `$PATH`: see `cargo_bin_dir`. If cargo itself cannot
    produce the binary (offline, registry down, name typo), this fails loudly
    with `on_install_failure_hint` attached -- the place to name the actual
    reason this project cannot substitute some other build of the same name
    (a different language runtime, a different major version), since that
    reason has nowhere else durable to live.
    """
    binary = cargo_bin_dir() / name
    if not (binary.is_file() and os.access(binary, os.X_OK)):
        subprocess.run(["cargo", "install", name, "--locked"], cwd=ROOT)
    if not (binary.is_file() and os.access(binary, os.X_OK)):
        hint = f" {on_install_failure_hint}" if on_install_failure_hint else ""
        raise SystemExit(
            f"gate_diff: {name} is not at {binary} and `cargo install {name} "
            f"--locked` did not put it there.{hint}"
        )
    return binary


def intersects(a: tuple[int, int], b: tuple[int, int]) -> bool:
    """Whether two inclusive `(start, end)` line ranges overlap."""
    return a[0] <= b[1] and b[0] <= a[1]


def any_intersect(ranges: list[tuple[int, int]], span: tuple[int, int]) -> bool:
    return any(intersects(r, span) for r in ranges)
