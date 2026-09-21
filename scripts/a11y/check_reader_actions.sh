#!/usr/bin/env bash
set -euo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"
output="${1:?new artifact directory}"
mkdir "$output"
exec >"$output/check.log" 2>&1
git status --short --branch
git rev-parse HEAD
scripts/ci/with_host_lock.sh --shared just test-reader-actions
scripts/ci/with_host_lock.sh --shared just build-accessibility-desktop
just robot-accessibility-linux target/ci/desktop-app "$output/native"
