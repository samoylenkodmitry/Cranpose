#!/bin/bash
# Builds, installs and launches the Cranpose iOS demo on a booted simulator.
#
# Usage:
#   ./run-sim.sh
#   SIMULATOR_DEVICE="iPhone 17 Pro" ./run-sim.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DEVICE="${SIMULATOR_DEVICE:-iPhone 17 Pro}"
BUNDLE_ID="io.cranpose.demo"

APP="$("$SCRIPT_DIR/build-app.sh" aarch64-apple-ios-sim)"

xcrun simctl boot "$DEVICE" 2>/dev/null || true
xcrun simctl bootstatus "$DEVICE"
xcrun simctl install "$DEVICE" "$APP"
xcrun simctl terminate "$DEVICE" "$BUNDLE_ID" 2>/dev/null || true
xcrun simctl launch "$DEVICE" "$BUNDLE_ID"
open -a Simulator
