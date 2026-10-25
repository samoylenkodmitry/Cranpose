#!/usr/bin/env bash
# The public API that each published crate this branch changed gains or loses
# against the merge base, from cargo-public-api. A report for review: it never
# fails the job. About 30 seconds for each changed crate.
#
#   public_api_diff.sh [base]      # base defaults to origin/main
set -uo pipefail

base_ref="${1:-origin/main}"
toolchain="nightly-2026-10-03"
summary="${GITHUB_STEP_SUMMARY:-/dev/stdout}"

merge_base() { git merge-base HEAD "$base_ref" 2>/dev/null; }
if ! base="$(merge_base)"; then
  git fetch -q --no-tags origin main 2>/dev/null
  base="$(merge_base)" || { git fetch -q --no-tags --unshallow origin 2>/dev/null; base="$(merge_base)"; } || {
    echo "public-api: no merge base with $base_ref; nothing to report"
    exit 0
  }
fi

crates="$(git diff --name-only "$base" HEAD | python3 -c '
import json, pathlib, subprocess, sys
changed = [line.strip() for line in sys.stdin if line.strip()]
metadata = json.loads(subprocess.run(
    ["cargo", "metadata", "--no-deps", "--format-version", "1"],
    capture_output=True, text=True, check=True).stdout)
root = pathlib.Path(metadata["workspace_root"])
for package in metadata["packages"]:
    if package.get("publish") == []:
        continue
    directory = pathlib.Path(package["manifest_path"]).parent.relative_to(root).as_posix()
    if not directory.startswith("crates/"):
        continue
    if any(path.startswith(directory + "/src/") or path == directory + "/Cargo.toml" for path in changed):
        print(package["name"])
')"

if [ -z "$crates" ]; then
  echo "public-api: no published crate changed its sources"
  exit 0
fi

{
  echo "## Public API changes against ${base:0:9}"
  for crate in $crates; do
    echo
    echo "### $crate"
    echo '```diff'
    cargo "+$toolchain" public-api -p "$crate" diff "$base..HEAD" 2>/dev/null \
      || echo "(cargo-public-api could not build $crate at both commits)"
    echo '```'
  done
} >> "$summary"
exit 0
