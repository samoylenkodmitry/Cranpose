#!/bin/sh
# Builds PerfSwiftUI.app here, as `desktop.py` runs it: -O with the whole module
# compiled at once, as Xcode's Release configuration does. Compiled one file
# at a time, the frame formulas in PerfData.swift could not inline into the
# views that call them.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
app=$here/build/PerfSwiftUI.app
mkdir -p "$app/Contents/MacOS"
cp "$here/Info.plist" "$app/Contents/Info.plist"
xcrun swiftc -O -wmo -parse-as-library -target arm64-apple-macos15 -o "$app/Contents/MacOS/PerfSwiftUI" \
    "$here/Gauntlet.swift" "$here/../shared-swift/PerfData.swift"
codesign --force --sign - "$app"
echo "$app"
