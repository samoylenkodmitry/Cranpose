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
#   PACKAGE=coroflow-demo BIN=coroflow-ios APP_NAME=CoroflowNotes \
#     INFO_PLIST=apps/coroflow-demo/ios/Info.plist ./build-app.sh   # another app
#
# Prints the path to the assembled `.app` bundle on stdout.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE="$(cd "$SCRIPT_DIR/../../.." && pwd)"
TARGET="${1:-aarch64-apple-ios-sim}"
PROFILE="${PROFILE:-debug}"
EXTRA_FEATURES="${EXTRA_FEATURES:-}"
PACKAGE="${PACKAGE:-desktop-app}"
BIN="${BIN:-cranpose-ios}"
APP_NAME="${APP_NAME:-CranposeDemo}"
INFO_PLIST="${INFO_PLIST:-$SCRIPT_DIR/CranposeDemo/Info.plist}"

case "$PROFILE" in
  release) PROFILE_FLAG="--release" ;;
  debug)   PROFILE_FLAG="" ;;
  *) echo "PROFILE must be 'debug' or 'release', got '$PROFILE'" >&2; exit 1 ;;
esac

# shellcheck disable=SC2086
cargo build --manifest-path "$WORKSPACE/Cargo.toml" \
  -p "$PACKAGE" --bin "$BIN" \
  --target "$TARGET" --no-default-features --features ios ${EXTRA_FEATURES:+--features "$EXTRA_FEATURES"} $PROFILE_FLAG >&2

BUILD_ROOT="${CARGO_TARGET_DIR:-$WORKSPACE/target}"
BINARY="$BUILD_ROOT/$TARGET/$PROFILE/$BIN"
APP="$BUILD_ROOT/$TARGET/$PROFILE/$APP_NAME.app"

if [[ -d "$APP" ]]; then
  PREVIOUS_APP="$(mktemp -d "$APP.previous.XXXXXX")"
  mv "$APP" "$PREVIOUS_APP/$APP_NAME.app"
fi
mkdir -p "$APP"
cp "$BINARY" "$APP/$APP_NAME"
cp "$INFO_PLIST" "$APP/Info.plist"

# Ad-hoc sign so the bundle runs on device/simulator without a developer team.
# Pass CODESIGN_IDENTITY for a real Developer ID / distribution identity.
codesign --force --sign "${CODESIGN_IDENTITY:--}" "$APP" >&2

echo "$APP"
