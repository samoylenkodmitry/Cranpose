#!/bin/sh
# Builds FrameCount.app into the given folder (default ~/Applications).
# Screen Recording permission belongs to the code's designated requirement. An
# ad-hoc signature's default one is the build's hash, which every rebuild
# changes, so the app names its bundle identifier instead.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
app=${1:-$HOME/Applications}/FrameCount.app
mkdir -p "$app/Contents/MacOS"
cp "$here/Info.plist" "$app/Contents/Info.plist"
xcrun swiftc -O -o "$app/Contents/MacOS/FrameCount" "$here/FrameCount.swift"
codesign --force --sign - \
    --requirements '=designated => identifier "dev.perfcompare.framecount"' "$app"
echo "$app"
