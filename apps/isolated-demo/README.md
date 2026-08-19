# Cranpose Isolated Demo

This is the standalone Cranpose starter project. It is a separate Cargo
workspace and depends on the published `cranpose` and `cranpose-core` crates
from crates.io, so it can be copied out of this repository without path
dependencies.

The demo shows a counter, state-driven accent-color changes, layout, buttons,
and the framework's embedded fallback font. Its supported targets are desktop,
Android, and web. iOS is intentionally not part of this template; the
repository's iOS reference is [`../ios-demo`](../ios-demo/README.md).

## Requirements

- Rust stable with the target needed for the platform you are building.
- A working native graphics environment for desktop builds.
- `cargo-ndk` and the Android SDK/NDK for Android builds.
- `wasm-pack` for web builds.

## Desktop

From this directory, run the demo with the default wgpu renderer:

```bash
cargo run --features desktop,renderer-wgpu,logging
```

The `desktop` and `renderer-wgpu` features are enabled by default; they are
shown explicitly here so the feature choices are clear.

## Android

Install the native build bridge once:

```bash
cargo install cargo-ndk
```

Then build from the Android project directory:

```bash
cd android
./gradlew :app:assembleDebug
```

The debug build produces the x86_64 library used by the emulator. Build the
release APK to package all configured ABIs:

```bash
./gradlew :app:assembleRelease
```

## Web (WASM)

Install the WebAssembly target and build tool:

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-pack
```

Build and serve the demo from this directory:

```bash
./build-web.sh
python3 -m http.server 8080
```

Open <http://localhost:8080>. The default browser backend is WebGL2. To force
WebGPU, open <http://localhost:8080/?backend=webgpu>; to force WebGL2, use
`?backend=gl`. Install Binaryen (`wasm-opt`) if you want the size optimizer
used by the web build to be available.

## Project layout

```text
apps/isolated-demo/
├── src/                 # Shared Cranpose app and platform entry points
├── android/             # Minimal Gradle host for the NativeActivity APK
├── build-web.sh         # wasm-pack build for the browser
├── index.html           # Canvas host page
├── Cargo.toml           # Standalone package and published dependencies
└── Cargo.lock
```

To start a new app from this template, copy this directory and change the
package metadata and the `cranpose` dependency in `Cargo.toml` as needed.
