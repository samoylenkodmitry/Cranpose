# Cranpose Isolated Demo

This demo is intentionally **isolated** from the repository workspace and depends only on
published crates from crates.io. It serves as the starter project template showing how to
build a Cranpose app for all supported platforms.

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

Open `ios/CranposeIsolatedDemo.xcodeproj` in Xcode, then build and run on a simulator or
device. The Xcode project invokes `cargo build` via a build phase script.

```bash
# Or build the Rust library manually:
cd apps/isolated-demo
cargo build --target aarch64-apple-ios-sim --lib --features ios,renderer-wgpu --no-default-features
```

## Web (WASM)

```bash
cd apps/isolated-demo
./build-web.sh
python3 -m http.server 8080
```

Open http://localhost:8080
