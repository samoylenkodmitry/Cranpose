# Cranpose Isolated Demo

This demo is intentionally **isolated** from the repository workspace and depends only on
published crates from crates.io. It is meant to validate the public API surface and
show a minimal multi-platform setup.

## Desktop

```bash
cd apps/isolated-demo
cargo run --features desktop,renderer-wgpu,logging
```

## Web

```bash
cd apps/isolated-demo
./build-web.sh
python3 -m http.server 8080
```

Open http://localhost:8080

## Android

```bash
cd apps/isolated-demo/android
./gradlew :app:assembleRelease
```

This uses `cargo-ndk` under the hood. Install it with:

```bash
cargo install cargo-ndk
```
