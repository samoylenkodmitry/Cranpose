#!/usr/bin/env python3
"""Counts that written rules keep low, checked so they only go down.

    source_budgets.py            # fail when a count differs from its budget
    source_budgets.py --lower    # also write the counts that fell below their budget

Each budget in source_budgets.toml is today's count of something a rule
forbids. A count above its budget fails and names what grew; a count below
it fails until the budget comes down in the same commit, so the saving stays.

- file_lines: a Rust source file outside tests has at most `limit` lines.
  The files over it are listed with their length, which only goes down.
- too_many_arguments: `clippy::too_many_arguments` suppressions. A function
  that would take 8 arguments takes a new type instead.
- private_docs: doc comment lines that `scripts/dev/strip_private_docs.py`
  would remove, on items that are not public API.
- for_test_fns: `fn *_for_test` and `fn *_for_tests` in library sources.
  A test drives the public API instead.
- cranpose_env_names: distinct `CRANPOSE_*` names quoted in library sources.
- source_text_checks: `.contains(` calls in tests under crates/ and apps/
  that read source files as text. Such a check breaks on a rename and passes
  a changed behavior. xtask reads text by design and is left out.
"""

import pathlib
import re
import subprocess
import sys
import tomllib

ROOT = pathlib.Path(__file__).resolve().parents[2]
BUDGET_FILE = ROOT / "scripts/ci/source_budgets.toml"
CODE_ROOTS = ("crates", "apps", "xtask")
SKIP_DIRS = {"target", "pkg", "node_modules", "build", ".gradle"}
FOR_TEST_FN = re.compile(r"\bfn\s+[a-z0-9_]+_for_tests?\b")
ENV_NAME = re.compile(r"\"(CRANPOSE_[A-Z0-9_]+)\"")
SOURCE_PATH = re.compile(r"\.(rs|kt|kts|swift|yml|yaml|xml|toml|gradle|py|sh|m|mm|h)\"")
READS_TEXT = re.compile(r"read_to_string\(|include_str!\(")


def rust_files(*roots: str) -> list[pathlib.Path]:
    files = []
    for root in roots:
        for path in (ROOT / root).rglob("*.rs"):
            if not SKIP_DIRS.intersection(path.relative_to(ROOT).parts):
                files.append(path)
    return sorted(files)


def is_test(path: pathlib.Path) -> bool:
    parts = path.relative_to(ROOT).parts
    return "tests" in parts or path.name.endswith("_tests.rs") or "benches" in parts


def is_library(path: pathlib.Path) -> bool:
    parts = path.relative_to(ROOT).parts
    return parts[0] == "crates" and "src" in parts and not is_test(path)


def rel(path: pathlib.Path) -> str:
    return str(path.relative_to(ROOT))


def measure() -> tuple[dict[str, int], dict[str, int]]:
    files = rust_files(*CODE_ROOTS)
    texts = {path: path.read_text(errors="replace") for path in files}
    lengths = {
        rel(path): text.count("\n") + (not text.endswith("\n") and bool(text))
        for path, text in texts.items()
        if not is_test(path)
    }
    too_many_arguments = sum(
        text.count("clippy::too_many_arguments") for path, text in texts.items() if not is_test(path)
    )
    for_test_fns = sum(
        len(FOR_TEST_FN.findall(text)) for path, text in texts.items() if is_library(path)
    )
    env_names = {
        name for path, text in texts.items() if is_library(path) for name in ENV_NAME.findall(text)
    }
    source_text_checks = sum(
        text.count(".contains(")
        for path, text in texts.items()
        if is_test(path)
        and rel(path).startswith(("crates/", "apps/"))
        and READS_TEXT.search(text)
        and SOURCE_PATH.search(text)
    )
    private_docs = private_doc_lines(files)
    counts = {
        "too_many_arguments": too_many_arguments,
        "private_docs": private_docs,
        "for_test_fns": for_test_fns,
        "cranpose_env_names": len(env_names),
        "source_text_checks": source_text_checks,
    }
    return counts, lengths


def private_doc_lines(files: list[pathlib.Path]) -> int:
    result = subprocess.run(
        [sys.executable, str(ROOT / "scripts/dev/strip_private_docs.py"), "--check", *map(str, files)],
        capture_output=True, text=True, check=True,
    )
    return int(result.stdout.strip().splitlines()[-1].split()[0])


def write_budgets(config: dict) -> None:
    lines = [BUDGET_FILE.read_text().split("[counts]")[0].rstrip("\n"), "", "[counts]"]
    lines += [f"{name} = {value}" for name, value in config["counts"].items()]
    lines += ["", "[file_lines]", f"limit = {config['file_lines']['limit']}", "", "[file_lines.over]"]
    lines += [f'"{path}" = {length}' for path, length in sorted(config["file_lines"]["over"].items())]
    BUDGET_FILE.write_text("\n".join(lines) + "\n")


def main(argv: list[str]) -> int:
    lower = argv[1:] == ["--lower"]
    if argv[1:] and not lower:
        print(__doc__, file=sys.stderr)
        return 2
    config = tomllib.loads(BUDGET_FILE.read_text())
    counts, lengths = measure()
    failures = []
    lowered = False

    for name, count in counts.items():
        budget = config["counts"][name]
        if count > budget:
            failures.append(f"{name}: {count}, over the budget of {budget}")
        elif count < budget:
            lowered = True
            config["counts"][name] = count
            failures.append(f"{name}: {count}, under the budget of {budget}")

    limit = config["file_lines"]["limit"]
    over = config["file_lines"]["over"]
    for path, length in sorted(lengths.items()):
        allowed = over.get(path, limit)
        if length > allowed:
            reason = "grew past its listed length" if path in over else f"is over {limit} lines"
            failures.append(f"{path}: {length} lines, {reason} ({allowed}); split it first")
        elif path in over and length < allowed:
            lowered = True
            if length > limit:
                over[path] = length
            else:
                del over[path]
            failures.append(f"{path}: {length} lines, under its listed length of {allowed}")
    for path in [path for path in over if path not in lengths]:
        lowered = True
        del over[path]
        failures.append(f"{path}: listed but gone")

    if lower and lowered:
        write_budgets(config)
        print(f"source-budgets: lowered budgets in {BUDGET_FILE.relative_to(ROOT)}")
        failures = [failure for failure in failures if "under" not in failure and "gone" not in failure]
    for failure in failures:
        print(f"source-budgets: {failure}", file=sys.stderr)
    if failures and not lower:
        print("source-budgets: `just source-budgets-lower` writes the counts that fell", file=sys.stderr)
    if not failures:
        summary = ", ".join(f"{name} {count}" for name, count in counts.items())
        print(f"source-budgets: within budget ({summary}; {len(over)} files over {limit} lines)")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
