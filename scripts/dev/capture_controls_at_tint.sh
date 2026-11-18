#!/bin/bash
# Sets the simulator's system Liquid Glass tint, then captures every native
# control state at it: scripts/dev/capture_controls_at_tint.sh DEVICE PERCENT OUTPUT
set -euo pipefail
device="$1"
percent="$2"
output="$3"
destination="platform=iOS Simulator,id=$device"
mkdir "$output"
just liquid-reference-test "$destination" "$output/set-tint.xcresult" "SystemGlassTintTests/testSet$percent" never > "$output/set-tint.log" 2>&1
just liquid-reference-test "$destination" "$output/controls.xcresult" NativeControlTests never > "$output/controls.log" 2>&1
xcrun xcresulttool export attachments --path "$output/controls.xcresult" --output-path "$output/controls" > /dev/null
echo "captured native controls at $percent% tint in $output/controls"
