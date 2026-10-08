# Cranpose Isolated Demo

This is the standalone Cranpose starter project. It is a separate Cargo
workspace and depends only on published crates.io releases of `cranpose` and
its sibling crates, so it can be copied out of this repository without path
dependencies and still build.

Three screens, reached through a bottom navigation bar, cover common app
patterns: **Home** has a counter and toggled card; **Tasks** has a text field
and a `LazyColumn` where users add, remove and complete tasks; **Settings**
switches the app's [`Palette`](src/theme.rs) between light and dark colors.
`IsolatedDemoApp` owns the theme flag and task repository. `TasksScreen` uses a
route-scoped `TasksViewModel`, and the navigation host preserves its store
across tab switches. The supported targets are desktop, Android, and web. The
iOS demo lives in [`../ios-demo`](../ios-demo/README.md).

The core UI uses caller-provided colors, while `cranpose-liquid` provides a
scoped `LiquidTheme` API. This template defines an app-level [`Palette`](src/theme.rs)
and passes the palette to each screen.

## Requirements

- Rust stable with the target needed for the platform you are building.
- A working native graphics environment for desktop builds.
- `cargo-ndk` and the Android SDK/NDK for Android builds.
- `wasm-pack` for web builds.

## Dependencies beyond `cranpose`

`Cargo.toml` declares the published UI, foundation, core, flow and navigation
crates used by the app. The UI crate supplies `Scaffold`; the foundation crate
supplies `TextFieldState`. Flow and navigation crates support the route-scoped
view models. The release workflow runs `cargo xtask sync-isolated-demo` after
publication to update the framework dependencies together.

## Desktop

From this directory, run the demo with the default wgpu renderer:

```bash
cargo run --features desktop,renderer-wgpu,logging
```

The `desktop` and `renderer-wgpu` features are enabled by default; they are
shown explicitly here so the feature choices are clear.

## Android

The Android build is configured by the `dev.cranpose.android` Gradle plugin: it
runs the native build, chooses the ABIs and Cargo profiles, packages the `.so`,
and adds the framework's activity and manifest contributions. The application's
own build file states only its namespace, its Cargo package and its label.

`android/settings.gradle.kts` locates the `cranpose` package in Cargo's registry
cache and includes the Gradle plugin from the package directory. The
[Android plugin guide](../../crates/cranpose/android/README.md) describes this
setup for a custom application.

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
release APK to package the release ABIs:

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

Open <http://localhost:8080>. The page uses WebGPU where the browser offers it
and WebGL2 otherwise. Add `?backend=gl` or `?backend=webgpu` to the URL to force
one. Install Binaryen (`wasm-opt`) if you want the size optimizer
used by the web build to be available.

## Project layout

```text
apps/isolated-demo/
├── src/
│   ├── main.rs         # Desktop binary entry point
│   ├── lib.rs          # Android and web platform entry points
│   ├── app.rs          # Root composable: nav state, theme state, shell
│   ├── theme.rs        # Palette and text-style helpers
│   ├── fonts.rs        # Font bundle (empty: uses the embedded fallback)
│   └── screens/        # One module per screen; add new screens here
│       ├── home.rs
│       ├── tasks.rs
│       └── settings.rs
├── android/            # Gradle host; the Cranpose plugin configures it
├── build-web.sh        # wasm-pack build for the browser
├── index.html          # Canvas host page
├── Cargo.toml          # Standalone package and published dependencies
└── Cargo.lock
```

To start a new app from this template, copy this directory, change the
package metadata and the `cranpose*` dependency versions in `Cargo.toml`, then
replace the screens under `src/screens/`. [`TasksScreen`](src/screens/tasks.rs)
shows a route-scoped view model, lifecycle-aware state collection, text input,
and a lazy list.
