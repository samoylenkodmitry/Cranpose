#!/usr/bin/env bash
# Lists every named function in the given Rust files with the cyclomatic
# complexity the complexity gate measures (rust-code-analysis-cli), highest
# first, so a refactor can see which bodies may move under a new name and
# which must be split before the gate accepts them.
#
# usage: scripts/dev/function_complexity.sh <file.rs>...
set -euo pipefail

if [ "$#" -lt 1 ]; then
    echo "usage: $0 <file.rs>..." >&2
    exit 2
fi

for file in "$@"; do
    rust-code-analysis-cli -m -O json -p "$file" | jq -r '
        [.. | objects | select(.kind? == "function" and .name != "<anonymous>")]
        | sort_by(-.metrics.cyclomatic.sum)
        | .[]
        | "\(.metrics.cyclomatic.sum | floor)\t\(.name)\t'"$file"':\(.start_line)"'
done
