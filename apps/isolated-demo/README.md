# Cranpose Isolated Demo

This is the standalone Cranpose starter project. It is a separate Cargo
workspace and depends only on published crates.io releases of `cranpose` and
its sibling crates, so it can be copied out of this repository without path
dependencies and still build.

Three screens, reachable through a bottom navigation bar, cover the concerns
a real app needs and a bare counter does not: **Home** is state and layout (a
counter, a toggled card); **Tasks** is a text field and a `LazyColumn` backed
by app-owned list state (add, remove, and check off tasks); **Settings** is
the one flag that switches the whole app's [`Palette`](src/theme.rs) between
its light and dark constants. `dark_mode` and the task list both live in
[`IsolatedDemoApp`](src/app.rs), the composable at the top of the tree, and
are passed down into whichever screen is showing — hoisted there because both
must survive switching screens, which state remembered inside a screen would
not. Its supported targets are desktop, Android, and web. iOS is
intentionally not part of this template; the repository's iOS reference is
[`../ios-demo`](../ios-demo/README.md).

Cranpose ships no theme system of its own (see the doc comment on
[`WearColors`](../../crates/cranpose-ui/src/widgets/wear/theme.rs) for why):
a `Palette` is an app-level struct a caller passes down explicitly, the same
way this template does.

## Requirements

- Rust stable with the target needed for the platform you are building.
- A working native graphics environment for desktop builds.
- `cargo-ndk` and the Android SDK/NDK for Android builds.
- `wasm-pack` for web builds.

## Dependencies beyond `cranpose`

`Cargo.toml` also depends directly on published `cranpose-ui` (for
`Scaffold`, the window-inset-aware app shell used for the top and bottom
bars) and `cranpose-foundation` (for `TextFieldState`, which `BasicTextField`
needs). Neither is re-exported by `cranpose::prelude`, so an app that wants a
system-inset-aware shell or a text field reaches for the crate that defines
it directly, the same way this one does. `scripts/sync_isolated_demo.py`
bumps every `cranpose*` dependency in this manifest together, so adding one
does not create a version to track by hand.

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

The plugin has no Maven coordinate. `android/settings.gradle.kts` locates the
`cranpose` crate source Cargo already resolved — the crates.io registry cache,
here — and includes the plugin straight from it, so there is nothing to
publish or pre-seed first; see [the crate's
README](../../crates/cranpose/README.md#android-gradle-plugin) for what that
`settings.gradle.kts` block does and how to copy it into a new application.

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

Open <http://localhost:8080>. The default browser backend is WebGL2. To force
WebGPU, open <http://localhost:8080/?backend=webgpu>; to force WebGL2, use
`?backend=gl`. Install Binaryen (`wasm-opt`) if you want the size optimizer
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
package metadata and the `cranpose*` dependency versions in `Cargo.toml` as
needed, then replace the screens under `src/screens/` with your own —
`rememberTasksState` in `src/screens/tasks.rs` is a worked example of a
`#[cfg(test)]` module exercising a composable's state through
`cranpose_ui::run_test_composition`, which is the pattern to follow for any
new state you add.
