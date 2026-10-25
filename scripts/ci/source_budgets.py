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
- half_state_words: lines in crates/, apps/, docs/ and README.md that speak
  of a state between two designs (TODO, FIXME, legacy, workaround...). Text
  describes the current design.
- ast_grep: the sites of each structural rule in rules/ast-grep, from one
  `ast-grep scan` of crates/, apps/ and xtask/. A rule with no budget fails.
"""

import collections
import json
import os
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
HALF_STATE_ROOTS = ("crates", "apps", "docs")
HALF_STATE_SUFFIXES = {".rs", ".md", ".toml", ".sh"}
HALF_STATE = re.compile(
    r"todo:|todo!\(|fixme|legacy|old way|when implemented|migration|workaround|backwards? compat",
    re.IGNORECASE,
)


def rust_files(*roots: str) -> list[pathlib.Path]:
    files = []
    for root in roots:
        for path in (ROOT / root).rglob("*.rs"):
            if not SKIP_DIRS.intersection(path.relative_to(ROOT).parts):
                files.append(path)
    return sorted(files)


def half_state_sites() -> list[str]:
    files = [ROOT / "README.md"]
    for root in HALF_STATE_ROOTS:
        for directory, subdirectories, names in os.walk(ROOT / root):
            subdirectories[:] = [name for name in subdirectories if name not in SKIP_DIRS]
            files += [pathlib.Path(directory, name) for name in names]
    sites = []
    for path in sorted(files):
        if path.suffix not in HALF_STATE_SUFFIXES:
            continue
        for number, line in enumerate(path.read_text(errors="replace").splitlines(), 1):
            if HALF_STATE.search(line):
                sites.append(f"{rel(path)}:{number}")
    return sites


def is_test(path: pathlib.Path) -> bool:
    parts = path.relative_to(ROOT).parts
    return "tests" in parts or path.name.endswith("_tests.rs") or "benches" in parts


def is_library(path: pathlib.Path) -> bool:
    parts = path.relative_to(ROOT).parts
    return parts[0] == "crates" and "src" in parts and not is_test(path)


def rel(path: pathlib.Path) -> str:
    return str(path.relative_to(ROOT))


def measure() -> tuple[dict[str, int], dict[str, int], dict[str, list[str]]]:
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
    sites = {"half_state_words": half_state_sites()}
    counts = {
        "too_many_arguments": too_many_arguments,
        "private_docs": private_docs,
        "for_test_fns": for_test_fns,
        "cranpose_env_names": len(env_names),
        "source_text_checks": source_text_checks,
        "half_state_words": len(sites["half_state_words"]),
    }
    return counts, lengths, sites


def private_doc_lines(files: list[pathlib.Path]) -> int:
    result = subprocess.run(
        [sys.executable, str(ROOT / "scripts/dev/strip_private_docs.py"), "--check", *map(str, files)],
        capture_output=True, text=True, check=True,
    )
    return int(result.stdout.strip().splitlines()[-1].split()[0])


def ast_grep_matches() -> list[dict]:
    result = subprocess.run(
        ["ast-grep", "scan", *CODE_ROOTS, "--json=compact"],
        cwd=ROOT, capture_output=True, text=True,
    )
    if result.returncode != 0:
        raise SystemExit(f"source-budgets: ast-grep scan failed\n{result.stderr}")
    return json.loads(result.stdout)


def report_rule_sites(matches: list[dict], budgets: dict[str, int], counts: dict[str, int]) -> None:
    for rule, count in sorted(counts.items()):
        if count <= budgets.get(rule, 0):
            continue
        sites = [match for match in matches if match["ruleId"] == rule]
        print(f"source-budgets: {rule}: {sites[0]['message']}", file=sys.stderr)
        for match in sites[:20]:
            print(f"    {match['file']}:{match['range']['start']['line'] + 1}", file=sys.stderr)


def compare(measured: dict[str, int], budgets: dict[str, int], failures: list[str]) -> bool:
    lowered = False
    for name in sorted(set(measured) | set(budgets)):
        count = measured.get(name, 0)
        if name not in budgets:
            failures.append(f"{name}: {count}, and no budget for it")
            continue
        budget = budgets[name]
        if count > budget:
            failures.append(f"{name}: {count}, over the budget of {budget}")
        elif count < budget:
            lowered = True
            budgets[name] = count
            failures.append(f"{name}: {count}, under the budget of {budget}")
    return lowered


def write_budgets(config: dict) -> None:
    lines = [BUDGET_FILE.read_text().split("[counts]")[0].rstrip("\n"), "", "[counts]"]
    lines += [f"{name} = {value}" for name, value in config["counts"].items()]
    if "ast_grep" in config:
        lines += ["", "[ast_grep]"]
        lines += [f'"{rule}" = {value}' for rule, value in sorted(config["ast_grep"].items())]
    lines += ["", "[file_lines]", f"limit = {config['file_lines']['limit']}", "", "[file_lines.over]"]
    lines += [f'"{path}" = {length}' for path, length in sorted(config["file_lines"]["over"].items())]
    BUDGET_FILE.write_text("\n".join(lines) + "\n")


def main(argv: list[str]) -> int:
    lower = argv[1:] == ["--lower"]
    if argv[1:] and not lower:
        print(__doc__, file=sys.stderr)
        return 2
    config = tomllib.loads(BUDGET_FILE.read_text())
    counts, lengths, sites = measure()
    failures: list[str] = []
    lowered = compare(counts, config["counts"], failures)
    for name, found in sites.items():
        if len(found) > config["counts"].get(name, 0):
            print(f"source-budgets: {name}:", *found[:20], sep="\n    ", file=sys.stderr)
    if "ast_grep" in config:
        matches = ast_grep_matches()
        rule_counts = dict(collections.Counter(match["ruleId"] for match in matches))
        report_rule_sites(matches, config["ast_grep"], rule_counts)
        lowered |= compare(rule_counts, config["ast_grep"], failures)

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
