# iOS Demo

Cranpose runs on iOS through a winit-based UIKit backend (`winit-uikit`). It
provides a real `UIWindow`/`UIView` backed by a `CAMetalLayer`, CADisplayLink
redraw scheduling, touch input, density (scale factor), the application
lifecycle, and safe-area insets — it is not a reuse of the desktop window
backend.

The demo binary is the shared `desktop-app` crate built with the `ios` feature
(target `aarch64-apple-ios-sim` for the Simulator, `aarch64-apple-ios` for a
device). winit starts `UIApplicationMain`, so the app is a pure-Rust binary
(`cranpose-ios`) with no Objective-C entry point and no Xcode project.

## Prerequisites

```bash
rustup target add aarch64-apple-ios-sim aarch64-apple-ios
```

Xcode (for the iOS SDK and Simulator runtimes) must be installed.

## Run on the Simulator

```bash
# Boots the default simulator, builds, installs and launches the app.
./ios/run-sim.sh

# Pick a specific simulator:
SIMULATOR_DEVICE="iPhone 17 Pro" ./ios/run-sim.sh
```

## Build a `.app` bundle

```bash
# Simulator bundle (debug):
./ios/build-app.sh aarch64-apple-ios-sim

# Device bundle (optimized, signed with your distribution identity):
PROFILE=release CODESIGN_IDENTITY="Apple Distribution: Your Team (TEAMID)" \
  ./ios/build-app.sh aarch64-apple-ios
```

`build-app.sh` prints the path to the assembled `CranposeDemo.app`. It is
ad-hoc signed by default (`-`); pass `CODESIGN_IDENTITY` to sign for device
installation. `CranposeDemo/Info.plist` is the bundle's property list.

## The scene manifest every app needs

iOS 27 ends an app at launch when the binary links against the iOS 27 SDK and
the app does not take up the UIScene life cycle. The message reads
`Application failed to launch: UIScene life cycle is required for apps built
with this SDK`. Cranpose ships the delegate class, so an app only declares it
in its own `Info.plist`:

```xml
<key>UIApplicationSceneManifest</key>
<dict>
    <key>UIApplicationSupportsMultipleScenes</key>
    <false/>
    <key>UISceneConfigurations</key>
    <dict>
        <key>UIWindowSceneSessionRoleApplication</key>
        <array>
            <dict>
                <key>UISceneConfigurationName</key>
                <string>Default Configuration</string>
                <key>UISceneDelegateClassName</key>
                <string>CranposeSceneDelegate</string>
            </dict>
        </array>
    </dict>
</dict>
```

`CranposeSceneDelegate` lives in `crates/cranpose/src/ios_scene.rs`. It takes
the window winit creates and hands it to the scene UIKit connects, in whichever
order the two arrive. An app without the manifest keeps the older path and
still runs on iOS 26 and below.

## Architecture

The backend lives in `crates/cranpose/src/ios.rs` and is selected by the `ios`
cargo feature on `target_os = "ios"`. It reuses the shared render stack
(`AppShell`, `WgpuRenderer`, wgpu Metal surface) and the winit input mapping,
publishing safe-area insets to composition via `local_safe_area_insets()`.
