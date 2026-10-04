#!/usr/bin/env python3
"""Report public function names missing from the test source corpus.

For each framework crate, this compares public function identifiers with names
in `#[cfg(test)]` modules, `tests/` files, and the desktop robot suite. The
report groups missing names by crate and gives a total name-reference ratio.
This source-text heuristic counts identifiers in comments; calls through a
caller count only when test source also names the public function.

The corpus includes every `#[cfg(test)] mod` body and `tests/` file under
`crates/`, plus the desktop robot examples and runners executed by
`./run_robot_test.sh`.

The corpus uses each test module's brace-delimited body. The scan excludes
adjacent production functions and test-only imports.

    python3 scripts/public_api_test_coverage.py            # the summary
    python3 scripts/public_api_test_coverage.py --list      # every name
    python3 scripts/public_api_test_coverage.py --crate cranpose-ui
"""

from __future__ import annotations

import argparse
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CRATES = ROOT / "crates"
ROBOT_SUITE = (
    ROOT / "apps" / "desktop-demo" / "examples",
    ROOT / "apps" / "desktop-demo" / "robot-runners",
    ROOT / "apps" / "desktop-demo" / "tests",
    ROOT / "apps" / "desktop-demo" / "src" / "tests",
)
PUBLIC_FN = re.compile(r"^\s*pub (?:const |async )?fn ([A-Za-z_][A-Za-z0-9_]*)", re.M)
TEST_MOD = re.compile(
    r"#\[cfg\(test\)\]\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+[A-Za-z_][A-Za-z0-9_]*\s*\{"
)


def split_test_modules(text: str) -> tuple[str, str]:
    """Separates a file into its production half and its `#[cfg(test)] mod` halves.

    Braces are matched so a test module in the middle of a file does not swallow
    what follows it, and string and comment contents are stepped over so a brace
    inside either does not move the boundary."""
    production: list[str] = []
    tests: list[str] = []
    cursor = 0
    for match in TEST_MOD.finditer(text):
        if match.start() < cursor:
            continue
        end = _matching_brace(text, match.end() - 1)
        if end is None:
            continue
        production.append(text[cursor : match.start()])
        tests.append(text[match.start() : end + 1])
        cursor = end + 1
    production.append(text[cursor:])
    return "".join(production), "\n".join(tests)


def _matching_brace(text: str, opening: int) -> int | None:
    """Index of the `}` closing the `{` at `opening`, or None if unbalanced."""
    depth = 0
    index = opening
    length = len(text)
    while index < length:
        character = text[index]
        if character == "/" and index + 1 < length:
            following = text[index + 1]
            if following == "/":
                index = text.find("\n", index)
                if index < 0:
                    return None
                continue
            if following == "*":
                closing = text.find("*/", index + 2)
                if closing < 0:
                    return None
                index = closing + 2
                continue
        elif character == '"':
            index = _skip_string(text, index)
            continue
        elif character == "'":
            # A lifetime, not a character literal, when no closing quote follows
            # within a few characters; skipping either way is safe.
            closing = _skip_char(text, index)
            if closing is not None:
                index = closing
                continue
        elif character == "{":
            depth += 1
        elif character == "}":
            depth -= 1
            if depth == 0:
                return index
        index += 1
    return None


def _skip_string(text: str, quote: int) -> int:
    index = quote + 1
    while index < len(text):
        if text[index] == "\\":
            index += 2
            continue
        if text[index] == '"':
            return index + 1
        index += 1
    return index


def _skip_char(text: str, quote: int) -> int | None:
    index = quote + 1
    limit = min(len(text), quote + 5)
    while index < limit:
        if text[index] == "\\":
            index += 2
            continue
        if text[index] == "'":
            return index + 1
        index += 1
    return None


def source_files() -> list[Path]:
    return [
        path
        for path in CRATES.rglob("*.rs")
        if "target" not in path.parts
    ]


def robot_suite_files() -> list[Path]:
    return [
        path
        for directory in ROBOT_SUITE
        if directory.is_dir()
        for path in directory.rglob("*.rs")
    ]


def test_corpus(files: list[Path]) -> str:
    """Everything that is test code: `#[cfg(test)]` tails, `tests/` files, and
    the headless robot suite."""
    parts: list[str] = []
    for path in files:
        text = path.read_text(errors="ignore")
        _, tests = split_test_modules(text)
        if tests:
            parts.append(tests)
        if "tests" in path.parts:
            parts.append(text)
    for path in robot_suite_files():
        parts.append(path.read_text(errors="ignore"))
    return "\n".join(parts)


def named_in(corpus: str, name: str) -> bool:
    """Whether the corpus names this function, and not merely a longer name that
    contains it. `with_timeout` is a substring of `exit_with_timeout`, so a
    substring test reports untested functions as covered."""
    return re.search(r"\b" + re.escape(name) + r"\b", corpus) is not None


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--list", action="store_true", help="print every name")
    parser.add_argument("--crate", help="only this crate")
    arguments = parser.parse_args()

    files = source_files()
    corpus = test_corpus(files)

    total = 0
    untested: dict[str, set[str]] = {}
    for path in files:
        if "tests" in path.parts:
            continue
        crate = path.relative_to(CRATES).parts[0]
        if arguments.crate and crate != arguments.crate:
            continue
        text = path.read_text(errors="ignore")
        body, _ = split_test_modules(text)
        for name in set(PUBLIC_FN.findall(body)):
            total += 1
            if not named_in(corpus, name):
                untested.setdefault(crate, set()).add(name)

    gap = sum(len(names) for names in untested.values())
    for crate in sorted(untested):
        names = sorted(untested[crate])
        print(f"{crate}: {len(names)}")
        if arguments.list:
            for name in names:
                print(f"    {name}")
    covered = total - gap
    share = (covered / total * 100.0) if total else 100.0
    print(f"\n{covered}/{total} public functions are named by a test ({share:.1f}%)")
    print(f"{gap} are not")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
