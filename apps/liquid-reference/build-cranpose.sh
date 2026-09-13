#!/bin/bash
set -euo pipefail

reference_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
reference_target="${1:-aarch64-apple-ios-sim}"
cd "$reference_root"
cargo build --locked --release -p desktop-app --bin cranpose-liquid-reference \
    --target "$reference_target" --no-default-features --features ios
reference_output="${CARGO_TARGET_DIR:-$reference_root/target}/$reference_target/release"
reference_bundle="$reference_output/CranposeLiquidReference.app"
mkdir -p "$reference_bundle"
cp "$reference_output/cranpose-liquid-reference" "$reference_bundle/CranposeLiquidReference"
python3 - "$reference_bundle" <<'PY'
import plistlib
import sys
from pathlib import Path

info = {
    "CFBundleIdentifier": "io.cranpose.liquid-cranpose",
    "CFBundleExecutable": "CranposeLiquidReference",
    "CFBundleName": "CranposeLiquidReference",
    "CFBundlePackageType": "APPL",
    "CFBundleVersion": "1",
    "CFBundleShortVersionString": "1.0",
    "MinimumOSVersion": "26.0",
    "CADisableMinimumFrameDurationOnPhone": True,
    "UILaunchScreen": {},
    "UIDeviceFamily": [1],
    "UISupportedInterfaceOrientations": ["UIInterfaceOrientationPortrait"],
}
with (Path(sys.argv[1]) / "Info.plist").open("wb") as output:
    plistlib.dump(info, output)
PY
codesign --force --sign "${CODESIGN_IDENTITY:--}" "$reference_bundle"
printf '%s\n' "$reference_bundle"
