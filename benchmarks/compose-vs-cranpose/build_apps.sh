#!/usr/bin/env bash
# Builds the gauntlet's apps on a Mac, as the nightly does on macm3:
#
#   build_apps.sh desktop        every desktop app, where desktop.py runs them,
#                                with desktop-versions.json naming their versions
#   build_apps.sh android DIR    every Android app but Cranpose's, as DIR/APP.apk,
#                                with DIR/versions.json naming their versions
#   build_apps.sh release TAG    Cranpose's desktop app at the release TAG, where
#                                desktop.py runs it as `cranpose-release`
#   build_apps.sh browser DIR    every app that targets the browser, as DIR/APP/index.html
#                                with DIR/versions.json naming their versions
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
# NuGet's packages, npm's cache and Go's. Each app's last build is kept there too,
# with a stamp of what it read: an app whose files, pin and toolchains did not
# change since is not built again, so a night builds only what moved.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
cache=${PERF_BUILD_CACHE:-$HOME/ci-cache/gauntlet}
mode=${1:?usage: build_apps.sh desktop | android DIR | release TAG | browser DIR}
mkdir -p "$cache"

export PATH="$cache/flutter/bin:/opt/homebrew/bin:$HOME/.cargo/bin:$PATH"
export DOTNET_ROOT=${DOTNET_ROOT:-$cache/dotnet}
export PATH="$DOTNET_ROOT:$PATH"
export DOTNET_NOLOGO=1 DOTNET_CLI_TELEMETRY_OPTOUT=1
export NUGET_PACKAGES=$cache/nuget
export npm_config_cache=$cache/npm
export GOMODCACHE=$cache/go/mod GOCACHE=$cache/go/build
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

# What every build runs: Rust, Flutter, .NET and its workloads, Xcode, Node
# and the JDKs. A change to any of them builds every app again.
toolchains() {
    {
        (cd "$here" && rustc -V)
        flutter --version --machine
        dotnet --version
        dotnet workload list
        xcodebuild -version
        node -v
        go version
        /usr/libexec/java_home -V
    } 2>&1 | shasum -a 256 | cut -c1-16
}

# The stamp of the PATHS a build reads, as the checkout has them now (moved
# pins included), and of the toolchains.
stamp() {
    (cd "$here" && git ls-files -s -- "$@" && git diff HEAD -- "$@" && echo "$tools") | shasum -a 256 | cut -c1-16
}

# usage: build NAME OUTPUT PATHS... -- COMMAND...
# Runs COMMAND, which makes OUTPUT (a file or a folder, from this folder),
# unless the last build of NAME read the same PATHS and toolchains: then puts
# that build's OUTPUT back. The stamp is taken again after COMMAND, as a pin
# `attempt` put back changed what the build read.
build() {
    local name=$1 output=$here/$2
    shift 2
    local paths=()
    while [[ $1 != -- ]]; do
        paths+=("$1")
        shift
    done
    shift
    local kept=$cache/built/$name
    if [[ -e $kept && $(cat "$kept.stamp" 2>/dev/null) == "$(stamp "${paths[@]}")" ]]; then
        echo "== $name: unchanged since its last build"
        mkdir -p "$(dirname "$output")"
        ditto "$kept" "$output"
        return
    fi
    step "$name"
    "$@"
    mkdir -p "$cache/built"
    rm -rf "$kept"
    ditto "$output" "$kept"
    stamp "${paths[@]}" > "$kept.stamp"
}

case $mode in
desktop)
    flutter_ready
    dotnet_ready
    tools=$(toolchains)
    # Cranpose's app reads the whole workspace, which moves every night.
    step cranpose
    rust_target cranpose-app
    cargo_app cranpose-app
    for app in egui slint iced gpui tauri dioxus xilem; do
        rust_target "$app-app"
        reads=("$app-app" perf-data)
        # Dioxus draws with the web page's CSS.
        if [[ $app == dioxus ]]; then
            reads+=(web-app/www/style.css)
        fi
        build "$app" "$app-app/target/release/perf-compare-$app" "${reads[@]}" -- \
            attempt "$app" cargo_app "$app-app"
    done
    build avalonia avalonia-app/bin/Release/net10.0/osx-arm64/publish avalonia-app shared-cs -- \
        attempt avalonia bash -c "cd '$here/avalonia-app' && dotnet publish -c Release -f net10.0 -p:TargetFrameworks=net10.0 -r osx-arm64"
    build fyne fyne-app/build/perf-compare-fyne fyne-app -- \
        attempt fyne bash -c "cd '$here/fyne-app' && go build -o build/perf-compare-fyne ."
    build swiftui swiftui-app/build/PerfSwiftUI.app swiftui-app shared-swift -- "$here/swiftui-app/build.sh"
    build appkit appkit-app/build/PerfAppKit.app appkit-app shared-swift -- "$here/appkit-app/build.sh"
    build flutter flutter-app/build/macos/Build/Products/Release/perf_flutter.app flutter-app -- \
        bash -c "cd '$here/flutter-app' && flutter build macos --release"
    build compose compose-desktop-app/build/compose/binaries/main/app/PerfCompose.app \
        compose-desktop-app shared-compose shared-kotlin -- \
        attempt compose-desktop bash -c "cd '$here/compose-desktop-app' && ./gradlew --no-daemon -q createDistributable"
    build web web-app/www/js web-app shared-ts -- \
        attempt web bash -c "cd '$here/web-app' && npm ci --no-audit --no-fund && npx tsc -p tsconfig.json"
    python3 "$here/versions.py" write "$here/desktop-versions.json" --platform desktop
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
    tools=$(toolchains)
    build compose-android compose-app/app/build/outputs/apk/release/app-release.apk \
        compose-app shared-compose shared-kotlin -- \
        attempt compose bash -c "cd '$here/compose-app' && ./gradlew --no-daemon -q :app:assembleRelease"
    cp "$here/compose-app/app/build/outputs/apk/release/app-release.apk" "$out/compose.apk"
    build views views-app/app/build/outputs/apk/release/app-release.apk views-app shared-kotlin -- \
        attempt views bash -c "cd '$here/views-app' && ./gradlew --no-daemon -q :app:assembleRelease"
    cp "$here/views-app/app/build/outputs/apk/release/app-release.apk" "$out/views.apk"
    build flutter-android flutter-app/build/app/outputs/flutter-apk/app-release.apk flutter-app -- \
        bash -c "cd '$here/flutter-app' && flutter build apk --release --target-platform android-arm64"
    cp "$here/flutter-app/build/app/outputs/flutter-apk/app-release.apk" "$out/flutter.apk"
    build rn rn-app/android/app/build/outputs/apk/release/app-release.apk rn-app shared-ts -- \
        attempt rn bash -c "cd '$here/rn-app' && npm ci --no-audit --no-fund && cd android && ./gradlew --no-daemon -q :app:assembleRelease"
    cp "$here/rn-app/android/app/build/outputs/apk/release/app-release.apk" "$out/rn.apk"
    # NativeScript signs its release build with a keystore it is handed: the
    # Android debug key, which Gradle signs the React Native build with.
    build nativescript nativescript-app/platforms/android/app/build/outputs/apk/release/app-release.apk \
        nativescript-app shared-ts -- \
        attempt nativescript bash -c "cd '$here/nativescript-app' && npm ci --no-audit --no-fund && npx ns build android --release --gradleArgs=-Pabis=arm64-v8a --key-store-path '$HOME/.android/debug.keystore' --key-store-password android --key-store-alias androiddebugkey --key-store-alias-password android"
    cp "$here/nativescript-app/platforms/android/app/build/outputs/apk/release/app-release.apk" "$out/nativescript.apk"
    build lynx lynx-app/app/build/outputs/apk/release/app-release.apk lynx-app shared-ts -- \
        attempt lynx bash -c "cd '$here/lynx-app/page' && npm ci --no-audit --no-fund && npm run build && cd .. && ./gradlew --no-daemon -q :app:assembleRelease"
    cp "$here/lynx-app/app/build/outputs/apk/release/app-release.apk" "$out/lynx.apk"
    build maui maui-app/bin/Release/net10.0-android/android-arm64/publish/dev.perfcompare.maui-Signed.apk \
        maui-app shared-cs -- \
        bash -c "cd '$here/maui-app' && dotnet publish -c Release -f net10.0-android -p:AndroidSdkDirectory='$ANDROID_HOME'"
    cp "$here/maui-app/bin/Release/net10.0-android/android-arm64/publish/dev.perfcompare.maui-Signed.apk" "$out/maui.apk"
    build avalonia-android avalonia-app/bin/Release/net10.0-android/android-arm64/publish/dev.perfcompare.avalonia-Signed.apk \
        avalonia-app shared-cs -- \
        attempt avalonia bash -c "cd '$here/avalonia-app' && dotnet publish -c Release -f net10.0-android -p:AndroidSdkDirectory='$ANDROID_HOME'"
    cp "$here/avalonia-app/bin/Release/net10.0-android/android-arm64/publish/dev.perfcompare.avalonia-Signed.apk" "$out/avalonia.apk"
    for app in egui slint; do
        rust_target "$app-app"
        build "$app-android" "$app-app/build/outputs/apk/release/$app-release.apk" "$app-app" perf-data rust-android -- \
            attempt "$app" bash -c "cd '$here/rust-android' && env -u CARGO_TARGET_DIR ./gradlew --no-daemon -q :$app:assembleRelease"
        cp "$here/$app-app/build/outputs/apk/release/$app-release.apk" "$out/$app.apk"
    done
    # Capacitor's Android build needs JDK 21.
    jdk21=$(/usr/libexec/java_home -v 21)
    build web-android web-app/android/app/build/outputs/apk/release/app-release.apk web-app shared-ts -- \
        attempt web bash -c "cd '$here/web-app' && npm ci --no-audit --no-fund && npm run build && cd android && ./gradlew --no-daemon -q -Dorg.gradle.java.home='$jdk21' :app:assembleRelease"
    cp "$here/web-app/android/app/build/outputs/apk/release/app-release.apk" "$out/web.apk"
    python3 "$here/versions.py" write "$out/versions.json" --platform android
    ;;
browser)
    out=${2:?usage: build_apps.sh browser DIR}
    dist=$here/browser-dist
    mkdir -p "$out" "$dist"
    flutter_ready
    tools=$(toolchains)
    # Cranpose's app reads the whole workspace, which moves every night.
    step cranpose-web
    rust_target cranpose-app
    (cd "$here/cranpose-app" && env -u CARGO_TARGET_DIR wasm-pack build --target web --release \
        --no-default-features --features web,renderer-wgpu)
    mkdir -p "$dist/cranpose"
    cp "$here/cranpose-app/index.html" "$dist/cranpose/index.html"
    ditto "$here/cranpose-app/pkg" "$dist/cranpose/pkg"
    # The page, with the Roboto files its style names.
    build web-browser browser-dist/web web-app shared-ts -- \
        attempt web bash -c "cd '$here/web-app' && npm ci --no-audit --no-fund && npx tsc -p tsconfig.json && rm -rf '$dist/web' && mkdir -p '$dist/web/fonts' && cp -R www/. '$dist/web/' && cp '$here/fonts/'*.ttf '$dist/web/fonts/'"
    ditto "$dist" "$out"
    python3 "$here/versions.py" write "$out/versions.json" --platform browser
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
    echo "usage: build_apps.sh desktop | android DIR | release TAG | browser DIR" >&2
    exit 2
    ;;
esac
