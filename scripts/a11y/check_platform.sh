#!/usr/bin/env bash
set -euo pipefail
export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:$PATH"
root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"
platform="${1:?web, web-build, android, android-build, ios, ios-build, macos, windows, or quality}"
output="${2:?new artifact directory}"
mkdir "$output"
exec >"$output/check.log" 2>&1
git status --short --branch
git rev-parse HEAD
case "$platform" in
  web|web-build)
    bash scripts/a11y/build_platform.sh web "$output/build"
    if [[ "$platform" == web ]]; then
      just robot-accessibility-web "$output/build/site" "$output/native"
    fi
    ;;
  android)
    : "${ANDROID_HOME:?set the Android SDK location}"
    export PATH="$ANDROID_HOME/platform-tools:$PATH"
    scripts/ci/with_host_lock.sh --shared just clippy-android
    ;;
  android-build)
    : "${ANDROID_HOME:?set the Android SDK location}"
    export PATH="$ANDROID_HOME/platform-tools:$PATH"
    just android-robot-build
    ;;
  ios)
    just clippy-ios
    ;;
  ios-build)
    just ios-sim
    ;;
  macos)
    just build-accessibility-desktop
    just robot-accessibility-macos target/ci/desktop-app "$output/native"
    ;;
  windows)
    scripts/ci/with_host_lock.sh --shared just clippy-windows
    ;;
  quality)
    scripts/ci/with_host_lock.sh --shared just clippy
    just fmt-check
    just test-android-accessibility-contract
    ;;
  *) exit 64 ;;
esac
