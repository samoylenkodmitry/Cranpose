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

# Nothing here stops a server it did not start. An earlier version of this
# script reclaimed the port -- `--stop-server` then `--start-server` -- when the
# wait timed out, on the theory that a foreign-version daemon was squatting it.
# That daemon no longer exists (the host's second sccache was retired the same
# evening), but the reclaim outlived its reason and killed two builds:
#
#   sccache: warning: The server looks like it shut down unexpectedly,
#   compiling locally instead
#
# A shared server under concurrent load is exactly what fails to answer inside
# a fixed timeout while being perfectly alive and mid-compile for another job,
# so a recovery path keyed on that timeout is a guard that lies under load. If
# the server will not answer, say so and stop; do not shoot it.
if ! wait_for_server; then
    echo "sccache did not answer within 30s; refusing to compile without it" >&2
    exit 1
fi

"$sccache_bin" --version
