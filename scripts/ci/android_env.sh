#!/usr/bin/env bash
set -euo pipefail

# Points a self-hosted runner's job at its machine-local Android SDK, NDK and
# build caches, through GITHUB_ENV and GITHUB_PATH. Every job that builds for
# Android runs this first, so the SDK paths live in one place.

# Match the host's runner-name prefix, not one runner's exact name: every
# runner registered on samarch-1 shares it (samarch-1-cranpose,
# samarch-1-cranpose-2, ...).
if [[ "$RUNNER_OS" == "Linux" && "$RUNNER_NAME" == samarch-1-cranpose* ]]; then
    sdk_root="${ANDROID_SDK_ROOT:-/home/s/develop/sdk}"
    cache_root=/home/s/ci-cache/cranpose
elif [[ "$RUNNER_OS" == "macOS" && -d /Volumes/files/sdk ]]; then
    sdk_root=/Volumes/files/sdk
    cache_root=/Volumes/files/ci-cache/cranpose
else
    sdk_root="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-$HOME/Library/Android/sdk}}"
    cache_root="$HOME/ci-cache/cranpose"
fi

# sccache is content-addressed, so every runner on a host can share it. A
# target directory is not: two jobs building into one would package each
# other's artifacts, so each runner gets its own.
target_dir="$cache_root/targets/$RUNNER_NAME"

test -d "$sdk_root"
mkdir -p "$cache_root/sccache" "$target_dir"
{
    echo "ANDROID_HOME=$sdk_root"
    echo "ANDROID_SDK_ROOT=$sdk_root"
    echo "ANDROID_NDK_HOME=$sdk_root/ndk/30.0.16248370"
    echo "SCCACHE_DIR=$cache_root/sccache"
    echo "CARGO_TARGET_DIR=$target_dir"
} >> "$GITHUB_ENV"
{
    echo "$(eval echo "~$(id -un)")/.cargo/bin"
    echo "$HOME/.local/bin"
    echo "$sdk_root/cmdline-tools/latest/bin"
    echo "$sdk_root/platform-tools"
} >> "$GITHUB_PATH"
