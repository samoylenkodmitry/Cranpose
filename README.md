
[WIP.webm](https://github.com/user-attachments/assets/00533605-aa9c-4555-896c-c939195e3dce)


# RS-Compose 

Compose-RS is a Jetpack Compose–inspired declarative UI framework. The repository accompanies the architectural proposal documented in [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) and provides crate scaffolding for the core runtime, procedural macros, UI primitives, and example applications.

## Examples

### Desktop

Run the interactive desktop example:
```bash
cargo run --bin desktop-app
```

### Android

Build and run the Android demo app:

1. Install prerequisites:
   ```bash
   cargo install cargo-ndk
   rustup target add aarch64-linux-android armv7-linux-androideabi i686-linux-android x86_64-linux-android
   ```

2. Set environment variables:
   ```bash
   export ANDROID_HOME=$HOME/Android/Sdk
   export ANDROID_NDK_HOME=$ANDROID_HOME/ndk/26.1.10909125
   ```

3. Build and install:
   ```bash
   cd apps/android-demo/android
   ./gradlew installDebug
   ```

For detailed Android build instructions, see [`apps/android-demo/README.md`](apps/android-demo/README.md).

### Web

Build and run the demo in your browser using WebGL2:

1. Install prerequisites:
   ```bash
   rustup target add wasm32-unknown-unknown
   cargo install wasm-pack
   ```

2. Build and run:
   ```bash
   cd apps/desktop-demo
   ./build-web.sh
   python3 -m http.server 8080
   ```

3. Open http://localhost:8080 in any modern browser (Chrome, Firefox, Edge, Safari)

For detailed web build instructions, see [`apps/desktop-demo/README.md`](apps/desktop-demo/README.md).

## Quick Start

### Desktop

```rust
use compose_app::AppLauncher;

fn main() {
    let _ = env_logger::try_init();
    AppLauncher::new()
        .with_title("My Compose App")
        .with_size(800, 600)
        .run(my_app);
}


#[composable]
fn my_app() {
    Text("Hello, Compose!");
}
```

### Android

```rust
use compose_app::AppLauncher;

#[no_mangle]
fn android_main(app: android_activity::AndroidApp) {
    AppLauncher::new()
        .with_title("My Compose App")
        .run(app, my_app);
}

#[composable]
fn my_app() {
    Text("Hello, Compose!");
}
```

### Web

```rust
use compose_app::AppLauncher;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub async fn run_app() -> Result<(), JsValue> {
    AppLauncher::new()
        .with_title("My Compose App")
        .with_size(800, 600)
        .run_web("canvas-id", my_app)
        .await
}

#[composable]
fn my_app() {
    Text("Hello, Compose!");
}
```

## Multi-Platform Example

The desktop demo (`apps/desktop-demo`) demonstrates running the **same codebase** on all three platforms (Desktop, Android, and Web). See [`apps/desktop-demo/README.md`](apps/desktop-demo/README.md) for detailed build instructions for each platform.

## Roadmap

See [`docs/ROADMAP.md`](docs/ROADMAP.md) for detailed progress tracking, implementation status, and upcoming milestones.

### Modifier Migration Status

The fluent modifier builders have landed, but the end-to-end migration is still underway. Pointer
and focus invalidation queues are not yet wired into the runtime, and legacy widget nodes are still
present in `crates/compose-ui/src/widgets/nodes`. Check [`NEXT_TASK.md`](NEXT_TASK.md) and
[`modifier_match_with_jc.md`](modifier_match_with_jc.md) for an up-to-date list of outstanding
work before claiming parity with Jetpack Compose.
## Contributing

This repository is currently a design playground; issues and pull requests are welcome for discussions, experiments, and early prototypes that move the Jetpack Compose–style experience forward in Rust.

## License

This project is available under the terms of the Apache License (Version 2.0). See [`LICENSE-APACHE`](LICENSE-APACHE) for the full license text.
