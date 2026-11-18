#!/bin/bash
# Runs the named liquid-reference UI tests in one xcodebuild and exports their
# attachments next to the result bundle:
# scripts/dev/capture_reference_tests.sh DEVICE OUTPUT TEST...
# TEST is Class or Class/testMethod. OUTPUT must not exist yet; keep it
# outside target/, which garbage collection sweeps.
set -euo pipefail
device="$1"
output="$2"
shift 2
mkdir "$output"
only=()
for test in "$@"; do
  only+=("-only-testing:LiquidReferenceTests/$test")
done
xcodebuild -project apps/liquid-reference/LiquidReference.xcodeproj -scheme LiquidReference \
  -configuration Release -destination "platform=iOS Simulator,id=$device" \
  -derivedDataPath target/liquid-reference -resultBundlePath "$output/tests.xcresult" \
  -parallel-testing-enabled NO -collect-test-diagnostics never "${only[@]}" test \
  > "$output/tests.log" 2>&1 || status=$?
xcrun xcresulttool export attachments --path "$output/tests.xcresult" --output-path "$output/attachments" > /dev/null
grep -E "Test Case .*(passed|failed)" "$output/tests.log" | sed -E 's/.*Test Case .-\[LiquidReferenceTests\.//' || true
exit "${status:-0}"
