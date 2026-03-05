#!/bin/bash
set -euo pipefail

# Build Rust static library for iOS
# Called by Xcode as a "Run Script" build phase.
#
# Xcode sets these environment variables:
#   CONFIGURATION: Debug / Release
#   PLATFORM_NAME: iphoneos / iphonesimulator
#   ARCHS: arm64 (device & Apple Silicon sim) / x86_64 (Intel sim)

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"

# Determine Rust target triple from Xcode env
if [ "${PLATFORM_NAME:-iphoneos}" = "iphonesimulator" ]; then
    RUST_TARGET="aarch64-apple-ios-sim"
else
    RUST_TARGET="aarch64-apple-ios"
fi

# Determine build profile
if [ "${CONFIGURATION:-Release}" = "Debug" ]; then
    PROFILE_FLAG=""
    PROFILE_DIR="debug"
else
    PROFILE_FLAG="--release"
    PROFILE_DIR="release"
fi

echo "Building Rust for target: $RUST_TARGET ($CONFIGURATION)"

# Ensure the target is installed
rustup target add "$RUST_TARGET" 2>/dev/null || true

cd "$PROJECT_ROOT"
cargo build \
    --target "$RUST_TARGET" \
    -p desktop-app \
    --lib \
    --features ios,renderer-wgpu \
    --no-default-features \
    $PROFILE_FLAG

# Copy the static library to where Xcode expects it
LIB_SRC="$PROJECT_ROOT/target/$RUST_TARGET/$PROFILE_DIR/libdesktop_app.a"
LIB_DST="$SCRIPT_DIR/libdesktop_app.a"
cp "$LIB_SRC" "$LIB_DST"
echo "Copied $LIB_SRC -> $LIB_DST"
