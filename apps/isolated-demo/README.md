# Cranpose Isolated Demo

This demo is intentionally **isolated** from the repository workspace and depends only on
published crates from crates.io. It serves as the starter project template showing how to
build a Cranpose app for supported platforms.

## Desktop (Linux/macOS/Windows)

```bash
cd apps/isolated-demo
cargo run --features desktop,renderer-wgpu,logging
```

## Android

```bash
# Prerequisites: cargo install cargo-ndk
cd apps/isolated-demo/android
./gradlew :app:assembleRelease
```

## iOS

iOS is unavailable until Cranpose has a real iOS platform crate with a CAMetalLayer
surface, CADisplayLink frame driver, UIKit lifecycle bridge, touch/keyboard bridge,
and safe-area/density updates.

## Web (WASM)

```bash
cd apps/isolated-demo
./build-web.sh
python3 -m http.server 8080
```

Open http://localhost:8080
