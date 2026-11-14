#!/bin/bash
# Builds Everbloom and assembles `Everbloom.app`, as the benchmark measures it.
#
# The iOS backend is winit-based, so winit starts `UIApplicationMain` itself:
# the app is one Rust binary with no Objective-C entry point and no Xcode
# project.
#
# Usage:
#   ios/build-app.sh [target]
#     target: aarch64-apple-ios (default, the device) | aarch64-apple-ios-sim
#   PROFILE=debug ios/build-app.sh ...      # unoptimized (default: release)
#   EXTRA_FEATURES=diag ios/build-app.sh    # the marker's check lines (never measured)
#   CODESIGN_IDENTITY=<identity> PROVISIONING_PROFILE=<file.mobileprovision> \
#     ios/build-app.sh                      # signed for an iPhone
#
# Without a provisioning profile the bundle is ad-hoc signed, which the
# Simulator runs. Prints the path of the bundle on stdout.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
TARGET="${1:-aarch64-apple-ios}"
PROFILE="${PROFILE:-release}"
# Keep in step with MinimumOSVersion in Info.plist.
export IPHONEOS_DEPLOYMENT_TARGET=17.0

case "$PROFILE" in
  release) PROFILE_FLAG="--release" ;;
  debug) PROFILE_FLAG="" ;;
  *) echo "PROFILE must be 'debug' or 'release', got '$PROFILE'" >&2; exit 1 ;;
esac
case "$TARGET" in
  aarch64-apple-ios) PLATFORM=iphoneos ;;
  aarch64-apple-ios-sim) PLATFORM=iphonesimulator ;;
  *) echo "target must be aarch64-apple-ios or aarch64-apple-ios-sim, got '$TARGET'" >&2; exit 1 ;;
esac

# shellcheck disable=SC2086
cargo build --manifest-path "$ROOT/Cargo.toml" --bin everbloom \
  --target "$TARGET" --no-default-features --features "ios${EXTRA_FEATURES:+,$EXTRA_FEATURES}" \
  $PROFILE_FLAG >&2

OUT="${CARGO_TARGET_DIR:-$ROOT/target}/$TARGET/$PROFILE"
APP="$OUT/Everbloom.app"
if [[ -d "$APP" ]]; then
  PREVIOUS="$(mktemp -d "$APP.previous.XXXXXX")"
  mv "$APP" "$PREVIOUS/Everbloom.app"
fi
mkdir -p "$APP"
cp "$OUT/everbloom" "$APP/Everbloom"
cp "$HERE/Info.plist" "$APP/Info.plist"

if [[ -n "${PROVISIONING_PROFILE:-}" ]]; then
  python3 "$HERE/sign.py" "$APP" --platform "$PLATFORM" \
    --identity "${CODESIGN_IDENTITY:?CODESIGN_IDENTITY names the signing identity}" \
    --profile "$PROVISIONING_PROFILE" >&2
else
  codesign --force --sign - "$APP" >&2
fi
echo "$APP"
