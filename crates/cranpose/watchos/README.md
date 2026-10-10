# Experimental watchOS host

The watchOS host runs an existing Rust composable through `cranpose::watchos::Application` and the software renderer. Cranpose supplies the SwiftUI presenter, Objective-C++ bridge and generated Rust runner. The app supplies its Rust UI, manifest and bundle identity.

## Build

The packager requires Python 3.11 or newer, Rust, Xcode and the watchOS SDK on macOS. Add the app entry point and bundle metadata to `Cargo.toml`:

```toml
[package.metadata.cranpose.watchos]
entry = "ui::OrbitApp"
bundle-id = "com.example.cranorbit.watch"
name = "Cranorbit"
```

A watch app that an iPhone app carries names that app's bundle id as
`companion` (or `--companion`); the watch bundle id must start with it and a
dot. Without it the watch app stands alone. Put the built `CranposeWatch.app`
in the iPhone app's `Watch` folder. With the `cranpose/wearable` feature on
both apps, `cranpose_services::wearable` links them through WatchConnectivity.
The reasons the app gives in its build script with
`cranpose_capabilities::declare`, such as the microphone's, go into the watch
app's `Info.plist`.

Run `build.py` from the resolved `cranpose` crate directory. Cargo metadata identifies this directory for a published dependency. Metadata supplies `--entry`, `--bundle-id` and `--name`, so a simulator build can use:

```sh
python3 watchos/build.py \
  --manifest /path/to/app/Cargo.toml \
  --output /path/to/watch-build
```

The default target is the watchOS simulator, with a watchOS 10 deployment
target. Use `--target device` for arm64 watches; the device target uses
watchOS 26 or later. Supply `--sign IDENTITY` to sign the bundle. Give each target
its own output directory. `--prepare-only` creates the generated runner
before Apple compilation.

For framework development, `--framework-source /path/to/Cranpose` points the
generated workspace at a checkout. Published app manifests use the resolved
published dependency. The generated runner starts with app features disabled;
pass `--features` to select app features.

## Validation

`python3 smoke.py BUNDLE EVIDENCE_DIRECTORY` installs and launches a built
simulator app, captures evidence and removes the simulator created for the
check. `just test-watchos` runs the portable renderer, runtime and packager
checks. `just watchos` runs the Apple build and simulator check.

Simulator checks cover startup and presentation. Measure performance and
battery use, and test native gestures, on a physical watch.
