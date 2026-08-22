#!/usr/bin/env python3
"""Which public framework functions are never named by a test.

The plan asks for a unit test behind every public function and method. This
reports what is left: for each framework crate, the public functions whose name
appears nowhere in a `#[cfg(test)]` module or a `tests/` file.

Naming is a coarse proxy for coverage — a name mentioned in a doc comment
inside a test module counts, and a function tested only through a caller does
not. It is deliberately coarse: it is cheap to run, it never goes stale, and it
points at the functions nobody has thought about, which is what the gap is.

The test corpus is every `#[cfg(test)] mod` body and `tests/` file under
`crates/`, plus the headless robot suite under `apps/desktop-demo`. The robot
examples and runners are test code that happens not to live in `crates/`: they
are what `./run_robot_test.sh` executes, and they are the only exercise a good
part of the robot driver API ever gets. Leaving them out reported that API as
untested when it is the most heavily executed code in the repository.

The corpus is the braces of each test module, not the tail of the file from the
first `#[cfg(test)]` onwards. That attribute also introduces test-only imports,
and 138 files in this workspace carry one near the top: taking the tail counted
their entire production body as test code, so every function in them read as
covered by virtue of defining itself.

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
