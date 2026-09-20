#!/usr/bin/env bash
set -euo pipefail
platform="${1:?desktop, web, or android}"
output="${2:?artifact directory}"
mkdir -p "$output"
cargo metadata --locked --format-version 1 > "$output/dependencies.json"
case "$platform" in
  desktop) just build-accessibility-desktop ;;
  web)
    just clippy-wasm
    just web
    apps/desktop-demo/package-web.sh "$output/site"
    ;;
  android) just android-robot-build ;;
  *) echo "Unknown platform: $platform" >&2; exit 64 ;;
esac
