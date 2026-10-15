#!/usr/bin/env python3
"""Writes the record of one runtime matrix run: the tree it checked, each
job's result (passed, failed, skipped or cancelled) and the SHA-256 of every
file the jobs kept as artifacts, so a release candidate carries what ran on
it and on which builds.

Usage: runtime_matrix_record.py --results <needs JSON> --artifacts <dir>
       --commit <sha> --output <dir>

The results are the workflow's `toJSON(needs)`. The record goes to
`<output>/record.json`, and a table of it to the file `GITHUB_STEP_SUMMARY`
names when it is set.
"""

import argparse
import hashlib
import json
import os
import sys
from pathlib import Path


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as file:
        for block in iter(lambda: file.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def artifact_hashes(root: Path) -> dict:
    """The hash of every file under `root`, by its path from `root`."""
    if not root.is_dir():
        return {}
    return {
        str(path.relative_to(root)): sha256(path)
        for path in sorted(root.rglob("*"))
        if path.is_file()
    }


def job_results(needs: dict) -> dict:
    return {job: details.get("result", "unknown") for job, details in sorted(needs.items())}


def summary_table(record: dict) -> str:
    lines = [
        f"### Runtime matrix on `{record['commit']}`",
        "",
        "| Job | Result |",
        "|---|---|",
    ]
    lines += [f"| {job} | {result} |" for job, result in record["jobs"].items()]
    lines += ["", f"{len(record['artifacts'])} artifact files hashed (record.json).", ""]
    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--results", required=True, help="the workflow's toJSON(needs)")
    parser.add_argument("--artifacts", required=True, type=Path)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()

    try:
        needs = json.loads(args.results)
    except json.JSONDecodeError as error:
        print(f"runtime_matrix_record: the results are not JSON: {error}", file=sys.stderr)
        return 1
    record = {
        "commit": args.commit,
        "jobs": job_results(needs),
        "artifacts": artifact_hashes(args.artifacts),
    }
    args.output.mkdir(parents=True, exist_ok=True)
    (args.output / "record.json").write_text(json.dumps(record, indent=2) + "\n")
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as file:
            file.write(summary_table(record))
    print(summary_table(record))
    return 0


if __name__ == "__main__":
    sys.exit(main())
