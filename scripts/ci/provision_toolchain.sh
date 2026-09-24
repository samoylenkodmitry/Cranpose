#!/usr/bin/env bash
set -euo pipefail

# Puts the toolchain on PATH for this step and every later one, and starts
# sccache.
#
# Both, and in this order. `GITHUB_PATH` only reaches LATER steps, and the
# `cargo install` below runs in THIS one -- so on a self-hosted runner whose
# service PATH does not already carry `~/.cargo/bin` the step dies with
# `cargo: command not found` and exit 127, having "provisioned" a PATH it
# never used itself.
#
# Resolve the account's real home rather than trusting $HOME: actions/checkout
# temporarily overrides HOME, and a step that runs while it is overridden
# prepends a .cargo/bin that does not exist. That is why the sccache line
# below survived for so long -- sccache was already on the runner's own PATH,
# so nothing needed `cargo` until another tool did, and then it failed with
# exit 127.
real_home="$(eval echo "~$(id -un)")"
export PATH="$real_home/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"
if [[ -n "${GITHUB_PATH:-}" ]]; then
    echo "$real_home/.cargo/bin" >> "$GITHUB_PATH"
fi

if ! command -v cargo >/dev/null; then
    echo "cargo not found. HOME=$HOME real_home=$real_home PATH=$PATH" >&2
    exit 1
fi

# CI runs the same recipes a developer runs, so a gate cannot mean one thing
# locally and another thing here.
command -v just >/dev/null || cargo install just --locked

# `cargo test` runs the workspace's test binaries one after another and pays
# every process start end to end: 181 binaries, 205s, of which about half was
# startup and two binaries. nextest runs them in parallel -- same 5452 tests in
# 62s. It does not run doctests, so `just test` still runs those with cargo.
command -v cargo-nextest >/dev/null \
    || cargo install cargo-nextest --locked --version 0.9.145

# One sccache server serves every job on a host, and it keeps the cache
# directory of whichever job started it. The robot and Android jobs name the
# Linux host's shared cache; a job that started the server without naming it
# would open a second 40G cache beside the first on a disk that is already
# nearly full.
if [[ "$(uname -s)" == Linux && -z "${SCCACHE_DIR:-}" && -d "$real_home/ci-cache/cranpose" ]]; then
    export SCCACHE_DIR="$real_home/ci-cache/cranpose/sccache"
    if [[ -n "${GITHUB_ENV:-}" ]]; then
        echo "SCCACHE_DIR=$SCCACHE_DIR" >> "$GITHUB_ENV"
    fi
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
"$SCRIPT_DIR/start_sccache.sh"
