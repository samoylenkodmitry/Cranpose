#!/usr/bin/env bash
# Regression test for docs_only_change.sh.
#
# A wrong `false` costs one redundant run. A wrong `true` skips the robot
# suite, the Android build and the iOS build on a change that could break any
# of them, and the board stays green while it does. That asymmetry is why the
# fail-safe branches are pinned here alongside the happy path.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
detector="$script_dir/docs_only_change.sh"
workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT

failures=0

# Build a pull request the way actions/checkout presents one: a merge commit
# whose first parent is the base and whose second is the branch head.
open_pr() {
    local branch="$1"
    git -C "$repo" checkout -q -b "$branch" main
}

merge_pr() {
    local branch="$1"
    git -C "$repo" checkout -q main
    git -C "$repo" merge -q --no-ff "$branch" -m "merge $branch"
}

check() {
    local label="$1" want="$2" got="$3"
    if [[ "$got" == "$want" ]]; then
        printf 'ok    %-42s %s\n' "$label" "$got"
    else
        printf 'FAIL  %-42s got %s, want %s\n' "$label" "$got" "$want"
        failures=$((failures + 1))
    fi
}

detect() {
    (cd "$repo" && GITHUB_EVENT_NAME="${1:-pull_request}" bash "$detector")
}

repo="$workdir/repo"
mkdir -p "$repo"
git -C "$repo" init -q -b main
git -C "$repo" config user.email ci@example.invalid
git -C "$repo" config user.name ci
mkdir -p "$repo/docs/render-reference" "$repo/crates/foo/src" "$repo/tools/api-surface/src"
echo prose > "$repo/docs/guide.md"
echo prose > "$repo/README.md"
echo 'fn main() {}' > "$repo/crates/foo/src/lib.rs"
echo 'pub fn helper() {}' > "$repo/crates/foo/src/helper.rs"
echo 'fn main() {}' > "$repo/tools/api-surface/src/main.rs"
printf 'not-really-a-png' > "$repo/docs/render-reference/contract.png"
git -C "$repo" add -A
git -C "$repo" commit -qm base

# A nested prose file is the case the filter exists for.
open_pr prose-nested
echo more >> "$repo/docs/guide.md"
git -C "$repo" commit -qam .
merge_pr prose-nested
check "nested .md only" true "$(detect)"

# Root-level prose too: TIME_WASTERS.md and AGENTS.md live there, and the
# `**/*.md` glob that looks like the obvious filter does not match them.
open_pr prose-root
echo more >> "$repo/README.md"
git -C "$repo" commit -qam .
merge_pr prose-root
check "root-level .md only" true "$(detect)"

# The shape that motivated the filter: prose plus a source file. The source
# file wins, because it is the half a build can observe.
open_pr prose-and-source
echo more >> "$repo/docs/guide.md"
echo '// tweak' >> "$repo/crates/foo/src/lib.rs"
git -C "$repo" commit -qam .
merge_pr prose-and-source
check "prose + .rs" false "$(detect)"

# tools/ is Rust that no gate currently builds. It still runs the full board:
# nothing pins it that way, and the cost of being wrong is one-sided.
open_pr tools-only
echo '// tweak' >> "$repo/tools/api-surface/src/main.rs"
git -C "$repo" commit -qam .
merge_pr tools-only
check "tools/ source only" false "$(detect)"

# Images under docs/ are not prose. The render reference is a fixture.
open_pr docs-image
printf 'changed' > "$repo/docs/render-reference/contract.png"
git -C "$repo" commit -qam .
merge_pr docs-image
check "docs/ image" false "$(detect)"

# Rename detection would report only the .md destination and read as prose.
open_pr rename-source-to-md
git -C "$repo" mv crates/foo/src/lib.rs crates/foo/src/lib.md
git -C "$repo" commit -qam .
merge_pr rename-source-to-md
check ".rs renamed to .md" false "$(detect)"

# Deleting a source file is not prose either.
open_pr delete-source
git -C "$repo" rm -q crates/foo/src/helper.rs
git -C "$repo" commit -qam .
merge_pr delete-source
check "source deletion" false "$(detect)"

# Workflow edits must run the workflows they edit.
open_pr workflow-edit
mkdir -p "$repo/.github/workflows"
echo 'name: x' > "$repo/.github/workflows/x.yml"
git -C "$repo" add -A
git -C "$repo" commit -qm .
merge_pr workflow-edit
check "workflow file" false "$(detect)"

# Events without a base to diff against never skip.
check "push event" false "$(detect push)"
check "workflow_dispatch event" false "$(detect workflow_dispatch)"

# A HEAD that is not a merge commit means the diff is unknowable, not empty.
git -C "$repo" checkout -q workflow-edit
check "HEAD is not a merge commit" false "$(detect)"
git -C "$repo" checkout -q main

# The step output the workflows read must actually be written.
output_file="$workdir/github_output"
: > "$output_file"
(cd "$repo" && GITHUB_EVENT_NAME=pull_request GITHUB_OUTPUT="$output_file" bash "$detector" >/dev/null)
check "step output written" "docs_only=false" "$(cat "$output_file")"

if [[ "$failures" -ne 0 ]]; then
    printf '\n%d case(s) failed\n' "$failures" >&2
    exit 1
fi
printf '\nall docs-only filter cases pass\n'
