#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
demo="$root/apps/native-demo"
build_root="${CARGO_TARGET_DIR:-$root/target}"
profile="${PROFILE:-dev}"
profile_dir="$profile"
if [[ "$profile" == dev ]]; then profile_dir=debug; fi
cd "$root"

bindings() {
    cargo build --locked -p cranpose-native-demo --features bindings --lib --bin native-demo-bindgen
    case "$(uname -s)" in
        Darwin) library="$build_root/debug/libcranpose_native_demo.dylib" ;;
        Linux) library="$build_root/debug/libcranpose_native_demo.so" ;;
        *) echo "Generate these mobile bindings on macOS or Linux" >&2; exit 1 ;;
    esac
    for language in swift kotlin; do
        "$build_root/debug/native-demo-bindgen" generate "$library" \
            --language "$language" --no-format --config "$demo/uniffi.toml" \
            --out-dir "$demo/generated/$language"
    done
}

case "${1:-}" in
    bindings) bindings ;;
    android)
        bindings
        cargo ndk --platform 24 -t "${NATIVE_DEMO_ABI:-arm64-v8a}" \
            -o "$demo/generated/jniLibs" build --locked -p cranpose-native-demo \
            --lib --profile "$profile"
        bash "$root/apps/android-demo/android/gradlew" -p "$demo/android" \
            --no-daemon :app:assembleRelease
        ;;
    android-test)
        serial="${2:?Pass an adb device serial}"
        bash "$root/apps/android-demo/android/gradlew" -p "$demo/android" \
            --no-daemon :app:assembleDebug :app:assembleDebugAndroidTest
        adb -s "$serial" install -r "$demo/android/app/build/outputs/apk/debug/app-debug.apk"
        adb -s "$serial" install -r "$demo/android/app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk"
        result="$(adb -s "$serial" shell am instrument -w dev.cranpose.nativehost.test/androidx.test.runner.AndroidJUnitRunner)"
        echo "$result"
        [[ "$result" == *"OK ("* ]]
        ;;
    ios)
        bindings
        target="${2:-aarch64-apple-ios-sim}"
        case "$target" in
            aarch64-apple-ios-sim) sdk=iphonesimulator; swift_target=arm64-apple-ios15.0-simulator ;;
            aarch64-apple-ios) sdk=iphoneos; swift_target=arm64-apple-ios15.0 ;;
            *) echo "Expected aarch64-apple-ios-sim or aarch64-apple-ios" >&2; exit 1 ;;
        esac
        cargo build --locked -p cranpose-native-demo --lib --target "$target" --profile "$profile"
        app="$build_root/$target/$profile_dir/CranposeNativeDemo.app"
        mkdir -p "$app"
        xcrun --sdk "$sdk" swiftc -swift-version 5 -parse-as-library \
            -target "$swift_target" -sdk "$(xcrun --sdk "$sdk" --show-sdk-path)" \
            -I "$demo/generated/swift" \
            -Xcc "-fmodule-map-file=$demo/generated/swift/CranposeDemoFFI.modulemap" \
            "$demo/generated/swift/CranposeDemo.swift" "$demo/ios/CranposeView.swift" "$demo/ios/App.swift" \
            "$build_root/$target/$profile_dir/libcranpose_native_demo.a" \
            -framework UIKit -framework WebKit -framework Metal -framework QuartzCore \
            -framework CoreGraphics -framework CoreText -framework Security -framework SystemConfiguration \
            -lc++ -o "$app/CranposeNativeDemo"
        cp "$demo/ios/Info.plist" "$app/Info.plist"
        codesign --force --sign "${CODESIGN_IDENTITY:--}" "$app"
        echo "$app"
        ;;
    *) echo "Usage: $0 bindings | android | android-test SERIAL | ios [aarch64-apple-ios-sim|aarch64-apple-ios]" >&2; exit 1 ;;
esac
