#!/usr/bin/env bash
# Builds the gauntlet's apps on a Mac, as the nightly does on macm3:
#
#   build_apps.sh desktop        every desktop app, where desktop.py runs them
#   build_apps.sh android DIR    every Android app but Cranpose's, as DIR/APP.apk
#
# Cranpose's Android app is not here: the nightly builds it on the machine the
# phone is attached to, signed by the key its earlier builds carry.
#
# Build caches live under PERF_BUILD_CACHE, which survives the checkout's
# cleaning between jobs: each Rust app's target (linked in as its `target`),
# .NET with its Android workload, NuGet's packages and npm's cache.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
cache=${PERF_BUILD_CACHE:-$HOME/ci-cache/gauntlet}
mode=${1:?usage: build_apps.sh desktop | android DIR}
mkdir -p "$cache"

export PATH="/opt/homebrew/bin:$HOME/.cargo/bin:$PATH"
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
jdk21=$(/usr/libexec/java_home -v 21)

dotnet_ready() {
    if [[ ! -x $DOTNET_ROOT/dotnet ]]; then
        curl -fsSL https://dot.net/v1/dotnet-install.sh -o "$cache/dotnet-install.sh"
        bash "$cache/dotnet-install.sh" --channel 10.0 --install-dir "$DOTNET_ROOT"
    fi
    if ! dotnet workload list | grep -q maui-android; then
        dotnet workload install maui-android
    fi
}

# Each Rust app builds into its own target in the cache; the checkout links it
# in, so the binaries sit where desktop.py and rust-android look.
rust_target() {
    mkdir -p "$cache/targets/$1"
    ln -sfn "$cache/targets/$1" "$here/$1/target"
}

step() {
    echo "== $1"
}

case $mode in
desktop)
    for app in cranpose-app egui-app slint-app iced-app gpui-app; do
        step "$app"
        rust_target "$app"
        (cd "$here/$app" && env -u CARGO_TARGET_DIR cargo build --release)
    done
    step avalonia
    dotnet_ready
    (cd "$here/avalonia-app" && dotnet publish -c Release -f net10.0 -p:TargetFrameworks=net10.0 -r osx-arm64)
    step swiftui
    "$here/swiftui-app/build.sh"
    step flutter
    (cd "$here/flutter-app" && flutter build macos --release)
    step compose
    (cd "$here/compose-desktop-app" && ./gradlew --no-daemon -q createDistributable)
    step web
    (cd "$here/web-app" && npm ci --no-audit --no-fund && npx tsc -p tsconfig.json)
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
    step compose
    (cd "$here/compose-app" && ./gradlew --no-daemon -q :app:assembleRelease)
    cp "$here/compose-app/app/build/outputs/apk/release/app-release.apk" "$out/compose.apk"
    step views
    (cd "$here/views-app" && ./gradlew --no-daemon -q :app:assembleRelease)
    cp "$here/views-app/app/build/outputs/apk/release/app-release.apk" "$out/views.apk"
    step flutter
    (cd "$here/flutter-app" && flutter build apk --release --target-platform android-arm64)
    cp "$here/flutter-app/build/app/outputs/flutter-apk/app-release.apk" "$out/flutter.apk"
    step rn
    (cd "$here/rn-app" && npm ci --no-audit --no-fund && cd android && ./gradlew --no-daemon -q :app:assembleRelease)
    cp "$here/rn-app/android/app/build/outputs/apk/release/app-release.apk" "$out/rn.apk"
    step maui
    dotnet_ready
    (cd "$here/maui-app" && dotnet publish -c Release -f net10.0-android -p:AndroidSdkDirectory="$ANDROID_HOME")
    cp "$here/maui-app/bin/Release/net10.0-android/android-arm64/publish/dev.perfcompare.maui-Signed.apk" "$out/maui.apk"
    step avalonia
    (cd "$here/avalonia-app" && dotnet publish -c Release -f net10.0-android -p:AndroidSdkDirectory="$ANDROID_HOME")
    cp "$here/avalonia-app/bin/Release/net10.0-android/android-arm64/publish/dev.perfcompare.avalonia-Signed.apk" "$out/avalonia.apk"
    step "egui, slint"
    rust_target egui-app
    rust_target slint-app
    (cd "$here/rust-android" && env -u CARGO_TARGET_DIR ./gradlew --no-daemon -q :egui:assembleRelease :slint:assembleRelease)
    cp "$here/egui-app/build/outputs/apk/release/egui-release.apk" "$out/egui.apk"
    cp "$here/slint-app/build/outputs/apk/release/slint-release.apk" "$out/slint.apk"
    step web
    (cd "$here/web-app" && npm ci --no-audit --no-fund && npm run build \
        && cd android && ./gradlew --no-daemon -q -Dorg.gradle.java.home="$jdk21" :app:assembleRelease)
    cp "$here/web-app/android/app/build/outputs/apk/release/app-release.apk" "$out/web.apk"
    ;;
*)
    echo "usage: build_apps.sh desktop | android DIR" >&2
    exit 2
    ;;
esac
