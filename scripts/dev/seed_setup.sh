#!/usr/bin/env bash
# Sets this machine's clone up for cargo-seed: the tool, the commands every
# seed is built with, and the branch seeds follow.
#
# The warm commands are what agents build first in a new worktree, the lint
# recipes' own commands with their flags: clippy keys its results on its
# arguments, and another flag would build another unit.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."

command -v cargo-seed >/dev/null 2>&1 || cargo install cargo-seed --locked

warm=(
    "python3 scripts/ci/clippy_budget.py --count"
    "cargo clippy -p cranpose --all-targets -- -D warnings"
    "cargo build --workspace"
    "cargo test --workspace --no-run"
    "just clippy-robot"
    "just clippy-wasm"
    "just clippy-android"
)
git config --unset-all seed.warm || true
for command in "${warm[@]}"; do
    git config --add seed.warm "$command"
done
git config seed.ref origin/main
# A seed of this workspace takes tens of gigabytes; a refresh needs room for
# the units it rebuilds and the incremental state they write.
git config seed.minFreeGb 40

# The seed the earlier Python seeder made stays the current one.
if ! git config --get seed.path >/dev/null && old="$(git config --get cranpose.seed)"; then
    git config seed.path "$old"
fi
git config --unset cranpose.seed || true
