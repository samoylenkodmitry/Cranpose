# Experimental watchOS host

This host builds Apple Watch simulator and arm64 device apps from an existing Rust composable.
The application supplies its library manifest, root function and bundle metadata.
Cranpose owns the SwiftUI presenter, Objective-C++ bridge and generated Rust runner;
no Apple source files or Xcode project are needed in the application repository.

The host uses `cranpose::watchos::Application` and the software renderer. Layout and
input stay in logical points; the renderer scales the picture to device pixels.
Touch, crown events, foreground changes and the frame clock reach the shared UI.
The timer stops when the scene becomes inactive, and unchanged frames do not create
new images. The presenter copies changed images because SwiftUI may retain them.

## Build a watch app

Use an approved Apple Silicon build host with Python 3.11+, Rust, Xcode and the
watchOS simulator SDK. Complete Xcode's first-launch setup and install a watchOS
simulator runtime before attempting to run the app. The default deployment target
is watchOS 10.0; the build target is `aarch64-apple-watchos-sim`.

An application can declare its identity and existing public root composable in Cargo:

```toml
[package.metadata.cranpose.watchos]
entry = "ui::OrbitApp"
bundle-id = "com.example.cranorbit.watch"
name = "Cranorbit"
```

Run `watchos/build.py` from the **resolved, published `cranpose` crate**. Its location
is the parent of Cranpose's `manifest_path` in `cargo metadata --locked --format-version 1`.
Only `--manifest` and `--output` are required when the metadata above is present.
The builder uses its own published version, preserving the app's dependency lock
when creating the generated runner. It never writes to the app's sources.

For framework development, use a Cranpose checkout:

```sh
export RUSTUP_TOOLCHAIN=$(sed -n 's/^channel = "\(.*\)"/\1/p' rust-toolchain.toml)
python3 crates/cranpose/watchos/build.py \
  --manifest /path/to/cranorbit/app/Cargo.toml \
  --entry ui::OrbitApp \
  --bundle-id com.example.cranorbit.watch \
  --name Cranorbit \
  --framework-source "$PWD" \
  --output /path/to/cranorbit-watch-build
```

The output is `CranposeWatch.app`, signed ad hoc for the simulator. The generated
Cargo workspace, lockfile, bridge and native files also remain in that output
directory. The application manifest and sources are untouched. Application default
features are disabled; select portable features with `--features` if needed.

`--framework-source` creates development overrides only inside the generated
workspace. It is for framework development. Published consumers omit that flag; `--cranpose-version VERSION` can explicitly
select a published release. Application
release manifests must not contain checkout or path overrides for Cranpose.

The script refuses an unrelated nonempty output directory. Repeating the same
configuration reuses the lockfile and build products. Configuration changes update
the generated workspace while retaining compatible dependency versions.

Add `--target device` to build `aarch64-apple-watchos` with the watchOS SDK. Use a
separate output directory for each target. Device bundles are unsigned unless
`--sign IDENTITY` is supplied; device provisioning and App Store submission remain
Apple developer account tasks. The device target requires **watchOS 26 or later on
arm64 watches** (Series 9 and later or Ultra 2 and later); older `arm64_32` watches
are not supported by this build command. See Apple's
[64-bit requirement](https://developer.apple.com/news/?id=zt8rydnt).

The application still supplies one shared Rust UI. Its Android host and watchOS
host are provided by Cranpose; no Swift or Objective-C++ source belongs in the app.

## Run in the simulator

Run these commands on the approved Apple build host after installing the watchOS
27 runtime. Keep this dedicated device set on its internal disk. On the test host,
CoreSimulator rejected device creation and capture output on the external cache
drive; an internal set worked without changing the existing simulator directories.

```sh
watch_device_set=/Users/Shared/CranposeWatchDevices
mkdir -p "$watch_device_set"
watch_device_id=$(xcrun simctl --set "$watch_device_set" create \
  "Cranpose watch prototype" \
  com.apple.CoreSimulator.SimDeviceType.Apple-Watch-Series-9-45mm \
  com.apple.CoreSimulator.SimRuntime.watchOS-27-0)
xcrun simctl --set "$watch_device_set" bootstatus "$watch_device_id" -b
xcrun simctl --set "$watch_device_set" install "$watch_device_id" \
  /path/to/cranorbit-watch-build/CranposeWatch.app
xcrun simctl --set "$watch_device_set" launch "$watch_device_id" com.example.cranorbit.watch
xcrun simctl --set "$watch_device_set" io "$watch_device_id" screenshot "$watch_device_set/cranorbit.png"
xcrun simctl --set "$watch_device_set" shutdown "$watch_device_id"
```

Allow the first launch to finish before capturing the screen. Reuse the printed
device identifier for later runs instead of creating a new simulator each time.

## CI

`just watchos` builds a small shared Rust composable for the simulator and arm64
device, installs the simulator bundle, verifies that it stays alive for 15 seconds,
and saves a screenshot. The `watchOS device and simulator` job runs this command
on pull requests and main. The unsigned device ZIP, simulator ZIP and launch
evidence are uploaded as the `watchos` artifact. `just ci-full` includes this gate.

`watchos/smoke.py BUNDLE EVIDENCE_DIRECTORY` performs the same simulator check for
consumer apps. It creates a private temporary device set on the internal disk and
shuts down and deletes only its own simulator. This checks native startup and
presentation; touch and crown behavior are covered separately by runtime tests.

## Portable checks and captures

On the approved Linux build host, run:

```sh
scripts/ci/with_host_lock.sh --shared just test-watchos
```

This runs the software renderer tests, shared runtime integration tests, Clippy and
packager tests. It is also a dependency of `just test-features`. The runtime tests
cover scaled drawing and touch targeting, crown forwarding, foreground suspension,
resize and avoiding presentation for unchanged frames.

`--prepare-only` creates the runner without requiring an Apple SDK. To inspect the
shared content through the same runtime on a desktop build host:

```sh
cd /path/to/cranorbit-watch-build/runner
cargo run --release --locked --bin snapshot -- menu.ppm 396 484 2.0 120
cargo run --release --locked --bin snapshot -- game.ppm 396 484 2.0 120 \
  --ob_debug --ob_level=1 --ob_autoplay
```

These are software-renderer captures, not Apple Watch screenshots or performance
measurements. The snapshot executable enables debug launch arguments; the watch
host does not. The native build compiles only the runner library.

## Current limits

The unchanged Cranorbit `OrbitApp` was installed and launched on a watchOS 27.0
Apple Watch Series 9 (45 mm) simulator on 2026-10-02. Its menu and animation were
verified in native simulator screenshots and a recording. This is still a
simulator prototype; that check does not establish physical-watch performance.

Touch and crown routing pass the portable integration tests. Native gesture
interaction remains unverified: the test host has the command-line simulator
runtime but no Simulator graphical application. Before a release, validate native
gestures and gameplay, then physical-watch
rendering, crown sensitivity, frame rate, memory, battery use and resume behavior.
App Store packaging and provisioning, app icons, audio, haptics, purchases, persistent
preferences, accessibility and native dialogs still need watchOS integration or
validation. Software rendering also needs comparison against the application's
required visuals. Device builds are compile checks; physical-watch behavior has not been validated.
