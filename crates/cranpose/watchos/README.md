# Experimental watchOS host

The watchOS host runs an existing Rust composable through `cranpose::watchos::Application`. Cranpose supplies the SwiftUI and SpriteKit presenter, Objective-C++ bridge and generated Rust runner. The app supplies its Rust UI, manifest and bundle identity.

watchOS gives an app no Metal of its own, so the GPU is reached through SpriteKit. Each frame the runtime hands the presenter the scene as sprites (`cranpose_render_pixels::sprites`): filled and stroked rounded rects, arcs and lines with a solid colour or a two-colour gradient travel as numbers to one shared `SKShader`, which works out the same coverage as the software renderer. Text, shadows, images and the shapes the shader does not cover are rasterized once on the CPU into textures that stay while the item stays; moving an item by whole pixels moves its sprite and keeps its texture. SpriteKit's frame loop drives the frames, and a node changes only when its sprite does. `Application::pixels` rasterizes the same scene on the CPU for snapshots and tests.

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
An `info-plist` table adds Info.plist entries of the app's own, for example
`info-plist = { UIBackgroundModes = ["audio"] }` to go on recording with the
screen off once recording started on screen. `icon` (or `--icon`) names a
square PNG of 1024 pixels, relative to the
manifest, which the watch shows in a circle. The reasons the app gives in its
build script with
`cranpose_capabilities::declare`, such as the microphone's, go into the watch
app's `Info.plist`.

Run `build.py` from the resolved `cranpose` crate directory. Cargo metadata identifies this directory for a published dependency. Metadata supplies `--entry`, `--bundle-id` and `--name`, so a simulator build can use:

```sh
python3 watchos/build.py \
  --manifest /path/to/app/Cargo.toml \
  --output /path/to/watch-build
```

The default target is the watchOS simulator, with a watchOS 10 deployment
target. Use `--target device` for arm64 watches; the device target defaults
to watchOS 27, the first version for which the App Store takes a watch app
built for arm64 alone (earlier versions also need arm64_32). A watchOS 26
target still installs on a watch and warns. Supply `--sign IDENTITY` to sign
the bundle. Give each target
its own output directory. `--prepare-only` creates the generated runner
before Apple compilation.

For framework development, `--framework-source /path/to/Cranpose` points the
generated workspace at a checkout. Published app manifests use the resolved
published dependency. The generated runner starts with app features disabled;
pass `--features` to select app features.

## Crown

The crown reaches the app through SwiftUI's `digitalCrownRotation`, at medium
sensitivity and without a stride: with a stride, SwiftUI steps the value and
settles it on a step after the hand stops, so the app saw the crown late and
saw it move after it stopped. WatchKit's crown sequencer, which a SwiftUI app
reaches through its root interface controller, delivered nothing on a watch
running watchOS 27.

## Sound and haptics

With the `cranpose/audio` feature, `cranpose_audio` plays through an
`AVAudioEngine` whose source node runs the mixer. The audio session is in the
ambient category: the sound mixes with other apps' audio and stays quiet while
the watch is silenced. The engine starts on the first sound and pauses after
the mixer goes idle, and a new one opens after the system stops it, for
instance when headphones connect.

`HapticFeedback` events map to WatchKit's taps. Vibrations and waveforms play
the click, the one tap that WatchKit plays without a tone: one click for a
vibration, and one where each buzz of a waveform starts. The click has a
single strength, so amplitudes have no effect on a watch.

## Heart rate

With the `cranpose/heart-rate` feature and `Use::heart_rate` in the build
script, `cranpose_services::heart_rate` reads the wearer's pulse through
HealthKit. A watch measures the pulse every few seconds only during a workout,
so reading starts a workout session, and stopping ends it and discards its
workout: nothing is saved to Health. The declaration writes its sentence under
`NSHealthShareUsageDescription` and `NSHealthUpdateUsageDescription`, because
a session needs permission to save workouts, and adds the `workout-processing`
background mode. The App ID needs the HealthKit capability, so that the
signing profile carries the HealthKit entitlements.

## Validation

`python3 smoke.py BUNDLE EVIDENCE_DIRECTORY` installs and launches a built
simulator app, captures evidence and removes the simulator created for the
check. `just test-watchos` runs the portable renderer, runtime and packager
checks. `just watchos` runs the Apple build and simulator check.

Simulator checks cover startup and presentation, and the simulator draws the
SpriteKit sprites on the Mac's GPU. Measure performance and battery use, and
test native gestures, on a physical watch.
