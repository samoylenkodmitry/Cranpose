#!/bin/sh
# Builds PerfSwiftUI.app here, optimized, as `desktop.py` runs it.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
app=$here/build/PerfSwiftUI.app
mkdir -p "$app/Contents/MacOS"
cp "$here/Info.plist" "$app/Contents/Info.plist"
xcrun swiftc -O -parse-as-library -target arm64-apple-macos15 -o "$app/Contents/MacOS/PerfSwiftUI" \
    "$here/Gauntlet.swift" "$here/PerfData.swift"
codesign --force --sign - "$app"
echo "$app"
