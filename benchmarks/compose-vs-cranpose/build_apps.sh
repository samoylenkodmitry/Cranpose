#!/usr/bin/env bash
# Builds the gauntlet's apps on a Mac, as the nightly does on macm3:
#
#   build_apps.sh desktop        every desktop app, where desktop.py runs them
#   build_apps.sh android DIR    every Android app but Cranpose's, as DIR/APP.apk,
#                                with DIR/versions.json naming their versions
#   build_apps.sh release TAG    Cranpose's desktop app at the release TAG, where
#                                desktop.py runs it as `cranpose-release`
#
# Cranpose's Android apps are not here: the nightly builds them on the Mac the
# phone is attached to, signed by the key its earlier builds carry.
#
# With PERF_BUMPS naming `versions.py bump`'s record, an app whose moved pin
# does not build has the pin put back and builds again.
#
# Build caches live under PERF_BUILD_CACHE, which survives the checkout's
# cleaning between jobs: each Rust app's target (linked in as its `target`),
# the Flutter SDK on its stable channel, .NET with its Android workload,
# NuGet's packages and npm's cache.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
cache=${PERF_BUILD_CACHE:-$HOME/ci-cache/gauntlet}
mode=${1:?usage: build_apps.sh desktop | android DIR | release TAG}
mkdir -p "$cache"

export PATH="$cache/flutter/bin:/opt/homebrew/bin:$HOME/.cargo/bin:$PATH"
export DOTNET_ROOT=${DOTNET_ROOT:-$cache/dotnet}
export PATH="$DOTNET_ROOT:$PATH"
export DOTNET_NOLOGO=1 DOTNET_CLI_TELEMETRY_OPTOUT=1
export NUGET_PACKAGES=$cache/nuget
export npm_config_cache=$cache/npm
# The Android SDK the apps' platforms are installed in.
if [[ -d $HOME/Library/Android/sdk/platforms ]]; then
    export ANDROID_HOME=$HOME/Library/Android/sdk
fi
export ANDROID_SDK_ROOT=${ANDROID_HOME:?}

step() {
    echo "== $1"
}

# Runs a build; when it fails after `versions.py bump` moved its pin, puts the
# pin back and runs it once more.
attempt() {
    local pin=$1
    shift
    if "$@"; then
        return
    fi
    if [[ -n ${PERF_BUMPS:-} ]] && python3 -c "import json, sys; moved = json.load(open(sys.argv[1])).get(sys.argv[2]); sys.exit(0 if moved and not moved['reverted'] else 1)" "$PERF_BUMPS" "$pin"; then
        python3 "$here/versions.py" revert "$pin" --record "$PERF_BUMPS"
        "$@"
    else
        return 1
    fi
}

# The latest stable Flutter, kept in the cache.
flutter_ready() {
    if [[ ! -d $cache/flutter ]]; then
        git clone -q --branch stable https://github.com/flutter/flutter.git "$cache/flutter"
    else
        git -C "$cache/flutter" pull -q --ff-only
    fi
    flutter --version
}

# The latest .NET 10 and MAUI Android workload.
dotnet_ready() {
    curl -fsSL https://dot.net/v1/dotnet-install.sh -o "$cache/dotnet-install.sh"
    bash "$cache/dotnet-install.sh" --channel 10.0 --install-dir "$DOTNET_ROOT"
    if dotnet workload list | grep -q maui-android; then
        dotnet workload update
    else
        dotnet workload install maui-android
    fi
}

# Each Rust app builds into its own target in the cache; the checkout links it
# in, so the binaries sit where desktop.py and rust-android look.
rust_target() {
    mkdir -p "$cache/targets/$1"
    ln -sfn "$cache/targets/$1" "$here/$1/target"
}

cargo_app() {
    (cd "$here/$1" && env -u CARGO_TARGET_DIR cargo build --release)
}

case $mode in
desktop)
    flutter_ready
    dotnet_ready
    for app in cranpose egui slint iced gpui; do
        step "$app"
        rust_target "$app-app"
        attempt "$app" cargo_app "$app-app"
    done
    step avalonia
    attempt avalonia bash -c "cd '$here/avalonia-app' && dotnet publish -c Release -f net10.0 -p:TargetFrameworks=net10.0 -r osx-arm64"
    step swiftui
    "$here/swiftui-app/build.sh"
    step flutter
    (cd "$here/flutter-app" && flutter build macos --release)
    step compose
    attempt compose-desktop bash -c "cd '$here/compose-desktop-app' && ./gradlew --no-daemon -q createDistributable"
    step web
    attempt web bash -c "cd '$here/web-app' && npm ci --no-audit --no-fund && npx tsc -p tsconfig.json"
    step framecount
    # A rebuild of the same source would only sign it again.
    framecount=$HOME/Applications/FrameCount.app
    source_hash=$(shasum -a 256 "$here/framecount/FrameCount.swift" | cut -d' ' -f1)
    if [[ $(cat "$framecount/Contents/Resources/source-sha256" 2>/dev/null) != "$source_hash" ]]; then
        "$here/framecount/build.sh"
    fi
    ;;
android)
    out=${2:?usage: build_apps.sh android DIR}
    mkdir -p "$out"
    flutter_ready
    dotnet_ready
    step compose
    attempt compose bash -c "cd '$here/compose-app' && ./gradlew --no-daemon -q :app:assembleRelease"
    cp "$here/compose-app/app/build/outputs/apk/release/app-release.apk" "$out/compose.apk"
    step views
    attempt views bash -c "cd '$here/views-app' && ./gradlew --no-daemon -q :app:assembleRelease"
    cp "$here/views-app/app/build/outputs/apk/release/app-release.apk" "$out/views.apk"
    step flutter
    (cd "$here/flutter-app" && flutter build apk --release --target-platform android-arm64)
    cp "$here/flutter-app/build/app/outputs/flutter-apk/app-release.apk" "$out/flutter.apk"
    step rn
    attempt rn bash -c "cd '$here/rn-app' && npm ci --no-audit --no-fund && cd android && ./gradlew --no-daemon -q :app:assembleRelease"
    cp "$here/rn-app/android/app/build/outputs/apk/release/app-release.apk" "$out/rn.apk"
    step maui
    (cd "$here/maui-app" && dotnet publish -c Release -f net10.0-android -p:AndroidSdkDirectory="$ANDROID_HOME")
    cp "$here/maui-app/bin/Release/net10.0-android/android-arm64/publish/dev.perfcompare.maui-Signed.apk" "$out/maui.apk"
    step avalonia
    attempt avalonia bash -c "cd '$here/avalonia-app' && dotnet publish -c Release -f net10.0-android -p:AndroidSdkDirectory='$ANDROID_HOME'"
    cp "$here/avalonia-app/bin/Release/net10.0-android/android-arm64/publish/dev.perfcompare.avalonia-Signed.apk" "$out/avalonia.apk"
    for app in egui slint; do
        step "$app"
        rust_target "$app-app"
        attempt "$app" bash -c "cd '$here/rust-android' && env -u CARGO_TARGET_DIR ./gradlew --no-daemon -q :$app:assembleRelease"
        cp "$here/$app-app/build/outputs/apk/release/$app-release.apk" "$out/$app.apk"
    done
    step web
    # Capacitor's Android build needs JDK 21.
    jdk21=$(/usr/libexec/java_home -v 21)
    attempt web bash -c "cd '$here/web-app' && npm ci --no-audit --no-fund && npm run build && cd android && ./gradlew --no-daemon -q -Dorg.gradle.java.home='$jdk21' :app:assembleRelease"
    cp "$here/web-app/android/app/build/outputs/apk/release/app-release.apk" "$out/web.apk"
    python3 "$here/versions.py" write "$out/versions.json" --platform android
    ;;
release)
    tag=${2:?usage: build_apps.sh release TAG}
    tree=$cache/release-tree
    if [[ -e $tree/.git ]]; then
        git -C "$tree" checkout -q --detach "$tag"
    else
        git -C "$here" worktree add -q --force --detach "$tree" "$tag"
    fi
    app=$tree/benchmarks/compose-vs-cranpose/cranpose-app
    # A release before the desktop gauntlet has no `cranpose-release` to run.
    if ! grep -q 'pub fn run_desktop' "$app/src/lib.rs" 2>/dev/null; then
        echo "$tag has no desktop gauntlet: no cranpose-release"
        rm -f "$here/cranpose-app/target-release"
        exit 0
    fi
    step "cranpose $tag"
    mkdir -p "$cache/targets/cranpose-release"
    (cd "$app" && CARGO_TARGET_DIR="$cache/targets/cranpose-release" cargo build --release)
    ln -sfn "$cache/targets/cranpose-release" "$here/cranpose-app/target-release"
    ;;
*)
    echo "usage: build_apps.sh desktop | android DIR | release TAG" >&2
    exit 2
    ;;
esac
