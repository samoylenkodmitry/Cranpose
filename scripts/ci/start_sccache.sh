#!/usr/bin/env bash
# One sccache binary, one server, for every job sharing a runner host.
#
# The jobs used to set `RUSTC_WRAPPER: sccache` and let PATH decide, but they do
# not build the same PATH: the Android job adds ~/.local/bin as well as
# ~/.cargo/bin, the robot jobs add only ~/.cargo/bin. samarch-1 accumulated
# sccache 0.17.0 in one and 0.16.0 in the other, both binding port 4226, so the
# second job to start lost the race and died with
#
#   sccache: error: Server startup failed: Address in use
#   ##[error]Process completed with exit code 143
#
# before a single robot example ran. Two robot jobs start together on that host,
# so the collision is the normal case, not the unlucky one. Resolve the wrapper
# to one absolute path here and export it, so every job on the host drives the
# same binary against the same daemon.
set -euo pipefail

home="$(eval echo "~$(id -un)")"
sccache_bin="$home/.cargo/bin/sccache"

if [ ! -x "$sccache_bin" ]; then
    # Unset the wrapper for the bootstrap build: cargo would otherwise try to
    # drive the sccache that is not installed yet.
    RUSTC_WRAPPER='' cargo install sccache --locked
fi

if [ -n "${GITHUB_ENV:-}" ]; then
    echo "RUSTC_WRAPPER=$sccache_bin" >> "$GITHUB_ENV"
fi

# --start-server is idempotent but not instantaneous, and that window is what
# bites: a second job starting while the first daemon is still binding sees the
# port taken and exits non-zero. Ask once, tolerate losing the race, then wait
# for the server to actually answer -- a compile that starts before it does is
# the failure this script exists to prevent.
wait_for_server() {
    local i
    for ((i = 0; i < 30; i++)); do
        if "$sccache_bin" --show-stats >/dev/null 2>&1; then
            return 0
        fi
        sleep 1
    done
    return 1
}

"$sccache_bin" --start-server >/dev/null 2>&1 || true

if ! wait_for_server; then
    # Either nothing came up, or the port is held by a daemon this binary will
    # not talk to -- a host that has accumulated a second sccache of another
    # version is exactly how this failure was found. Take the port back once,
    # rather than leaving every later job to lose the same race.
    echo "sccache did not answer; reclaiming the port" >&2
    "$sccache_bin" --stop-server >/dev/null 2>&1 || true
    "$sccache_bin" --start-server >/dev/null 2>&1 || true
    if ! wait_for_server; then
        echo "sccache still did not answer; refusing to compile without it" >&2
        exit 1
    fi
fi

"$sccache_bin" --version
