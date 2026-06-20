#!/bin/bash
# Builds the Cranpose iOS demo and assembles a `.app` bundle.
#
# The iOS backend is winit-based (`winit-uikit`), so winit starts
# `UIApplicationMain` itself: the app is a pure-Rust binary (`cranpose-ios`)
# with no Objective-C entry point and no Xcode project.
#
# Usage:
#   ./build-app.sh [target]
#     target: aarch64-apple-ios-sim (default) | aarch64-apple-ios (device)
#   PROFILE=release ./build-app.sh        # optimized build
#
# Prints the path to the assembled `.app` bundle on stdout.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE="$(cd "$SCRIPT_DIR/../../.." && pwd)"
TARGET="${1:-aarch64-apple-ios-sim}"
PROFILE="${PROFILE:-debug}"
APP_NAME="CranposeDemo"

case "$PROFILE" in
  release) PROFILE_FLAG="--release" ;;
  debug)   PROFILE_FLAG="" ;;
  *) echo "PROFILE must be 'debug' or 'release', got '$PROFILE'" >&2; exit 1 ;;
esac

# shellcheck disable=SC2086
cargo build --manifest-path "$WORKSPACE/Cargo.toml" \
  -p desktop-app --bin cranpose-ios \
  --target "$TARGET" --no-default-features --features ios $PROFILE_FLAG >&2

BIN="$WORKSPACE/target/$TARGET/$PROFILE/cranpose-ios"
APP="$WORKSPACE/target/$TARGET/$PROFILE/$APP_NAME.app"

rm -rf "$APP"
mkdir -p "$APP"
cp "$BIN" "$APP/$APP_NAME"
cp "$SCRIPT_DIR/CranposeDemo/Info.plist" "$APP/Info.plist"

# Ad-hoc sign so the bundle runs on device/simulator without a developer team.
# Pass CODESIGN_IDENTITY for a real Developer ID / distribution identity.
codesign --force --sign "${CODESIGN_IDENTITY:--}" "$APP" >&2

echo "$APP"
