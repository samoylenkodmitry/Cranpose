# Cranpose

<img width="1536" height="1024" alt="Cranpose" src="https://github.com/user-attachments/assets/2ce48dfe-a048-4b9d-8812-a0e4534691f8" />

Cranpose is a declarative UI framework for Rust, modelled on Jetpack Compose:
`#[composable]` functions, a slot-table runtime with fine-grained recomposition,
snapshot state, and a modifier-chain layout system. One Rust codebase targets
**desktop** (Linux, macOS, Windows), **Android** (including Wear OS),
**iOS**, and the **web** through WebAssembly, rendering through wgpu on all of
them.

**[Try the web demo in your browser](https://samoylenkodmitry.github.io/Cranpose/)** ·
[Releases](https://github.com/samoylenkodmitry/Cranpose/releases) ·
[crates.io](https://crates.io/crates/cranpose)

[v0.0.40.webm](https://github.com/user-attachments/assets/df50209b-abfd-426a-b79c-a51a9543b385)

> Pre-alpha. The API changes without deprecation cycles, and versions are not
> compatible with each other.

## Quick start

[`apps/isolated-demo`](apps/isolated-demo) is a complete starter project that
depends only on published crates — copy it rather than starting from scratch.

```bash
git clone https://github.com/samoylenkodmitry/cranpose.git
cd cranpose/apps/isolated-demo
cargo run --features desktop,renderer-wgpu
```

Or add the framework to an existing project:

```toml
[dependencies]
cranpose = { version = "0.1.88", features = ["desktop", "renderer-wgpu"] }
```

## Example

State, layout, and input, in the shape the framework actually has: composables
take a `Modifier`, a spec, and their content; state comes from `useState` and is
read with `.value()`.

```rust
#![allow(non_snake_case)] // #[composable] functions are CamelCase

use cranpose::prelude::*;

#[derive(Clone, PartialEq)]
struct Todo {
    text: String,
    done: bool,
}

#[composable]
fn TodoApp() {
    let todos = useState(|| {
        vec![
            Todo { text: "Buy milk".into(), done: false },
            Todo { text: "Walk the dog".into(), done: true },
        ]
    });

    Column(
        Modifier::empty().fill_max_size().padding(24.0),
        ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(12.0)),
        move || {
            Text("Todo", Modifier::empty(), TextStyle::default());

            for (index, todo) in todos.value().into_iter().enumerate() {
                Row(
                    Modifier::empty().fill_max_width().clickable(move |_| {
                        let mut next = todos.value();
                        next[index].done = !next[index].done;
                        todos.set(next);
                    }),
                    RowSpec::default().horizontal_arrangement(LinearArrangement::spaced_by(8.0)),
                    move || {
                        Text(
                            if todo.done { "[x]" } else { "[ ]" },
                            Modifier::empty(),
                            TextStyle::default(),
                        );
                        Text(todo.text.clone(), Modifier::empty(), TextStyle::default());
                    },
                );
            }

            Button(
                Modifier::empty().padding(10.0),
                ButtonSpec::default(),
                move || {
                    let mut next = todos.value();
                    let position = next.len() + 1;
                    next.push(Todo { text: format!("Item {position}"), done: false });
                    todos.set(next);
                },
                || {
                    Text("Add", Modifier::empty(), TextStyle::default());
                },
            );
        },
    );
}

fn main() {
    AppLauncher::new()
        .with_title("Todo")
        .with_size(420, 560)
        .try_run(TodoApp)
        .expect("launch the app");
}
```

A list that only composes what is on screen uses `LazyColumn` with
`remember_lazy_list_state()` from `cranpose_foundation::lazy` instead of the
`for` loop above.

## What is in the box

| Crate | What it is |
|---|---|
| `cranpose` | The facade apps depend on: platform runtimes, `AppLauncher`, prelude |
| `cranpose-core` | Slot table, recomposition, snapshot state, effects, coroutines |
| `cranpose-ui` / `cranpose-ui-layout` / `cranpose-ui-graphics` | Widgets, modifiers, measurement, geometry |
| `cranpose-foundation` | Gestures, pointer/rotary input, lazy lists, text buffers |
| `cranpose-animation` | Springs, tweens, transitions, `animate*AsState` |
| `cranpose-liquid` | Glass component library: iOS-26-style materials, spring motion |
| `cranpose-services` | HTTP, clipboard, share, notifications, file picker, haptics, purchases, camera, theme |
| `cranpose-audio` | Real-time audio (AAudio on Android/Wear OS, cpal on desktop) |
| `cranpose-storekit` | StoreKit 2 in-app purchases (iOS/macOS) |
| `cranpose-testing` | The robot harness that drives real windows in tests |

The composition runtime uses Slot Table V2: active groups live in preorder
group, payload, and node tables, and inactive retained branches are explicit
detached subtrees. The specification is
[`docs/cranpose_slot_table_v2_design.md`](docs/cranpose_slot_table_v2_design.md);
gap-table notes elsewhere are historical rationale only.

## Platform support

| Platform | Backend | Status |
|---|---|---|
| Linux x86_64 | Vulkan (GLES fallback opt-in) via wgpu | Supported; the GPU end-to-end suite runs here |
| macOS aarch64 | Metal via wgpu | Supported; builds, tests and `.app` bundles run in CI |
| Windows x86_64 | DX12/Vulkan via wgpu | Cross-built and released; not continuously exercised |
| Android / Wear OS | Vulkan/GLES via wgpu | Release APK build is checked in CI |
| iOS | UIKit/CAMetalLayer via `winit-uikit` | Simulator and device builds are checked in CI |
| Web (WASM) | WebGL2 (WebGPU opt-in via `?backend=webgpu`) | Demo build and Pages deploy are checked in CI |

Release binaries for the desktop platforms are attached to each
[release](https://github.com/samoylenkodmitry/Cranpose/releases).

## Building

### Desktop (Linux/macOS/Windows)

```bash
cd apps/isolated-demo
cargo run --features desktop,renderer-wgpu
```

macOS `.app` bundles come from the workspace task runner:

```bash
cargo xtask bundle-macos \
  --package desktop-app \
  --bin desktop-app \
  --app-name "Cranpose Demo" \
  --bundle-id io.cranpose.demo
```

Pass `--resources <dir>` to copy resources into `Contents/Resources`, and
`--sign-identity <id>` to run the explicit codesign step.

### Android

```bash
# Prerequisites: cargo install cargo-ndk
cd apps/isolated-demo/android
./gradlew :app:assembleRelease
```

### iOS

The app is a pure-Rust binary — winit starts `UIApplicationMain`, so there is no
Objective-C entry point and no Xcode project. See
[`apps/ios-demo/README.md`](apps/ios-demo/README.md).

```bash
rustup target add aarch64-apple-ios-sim aarch64-apple-ios
cd apps/ios-demo
./ios/run-sim.sh
```

### Web (WASM)

```bash
# Prerequisites: cargo install wasm-pack
cd apps/isolated-demo
./build-web.sh
python3 -m http.server 8080
```

## Binary size

Cranpose apps stay small when two things are set up right: the cargo profile and
the feature set.

**1. Add a tuned release profile to your app's `Cargo.toml`.** Cargo profiles
come from the top-level package, so the framework cannot set them for you.
Without this, a plain `cargo build --release` produces a binary several times
larger than necessary (no LTO, no stripping, unwinding kept):

```toml
[profile.release]
opt-level = 3         # or "z" to trade some runtime speed for size
lto = true
codegen-units = 1
strip = true
panic = "abort"
```

**2. Pick features deliberately.** The `cranpose` default feature set favours
out-of-the-box behaviour over size:

- `embedded-default-font` (default): embeds the ~1.3 MiB NotoSansMerged
  fallback so text renders even when the app provides no fonts. Apps that
  bundle fonts through `AppLauncher::with_fonts` should build with
  `default-features = false` to drop it.
- `renderer-wgpu-gles` (off by default): the GL/GLES fallback for desktop
  machines without a working Vulkan driver. Leaving it off removes the GLES
  half of wgpu and naga's GLSL writer. Android always compiles the GLES
  fallback; web always compiles WebGL.
- `desktop-x11` / `desktop-wayland`: `desktop` compiles both display-server
  backends; picking one drops the other's window and input stack.

```toml
[dependencies]
cranpose = { version = "0.1.88", default-features = false, features = [
    "desktop",        # or just "desktop-wayland" / "desktop-x11"
    "renderer-wgpu",
] }
```

**3. For the smallest binary**, build with the nightly-only pipeline (build-std
with a size-tuned std and immediate-abort panics):

```bash
cargo xtask dist-min --package my-app --bin my-app
```

Reference ladder for a minimal hello-world on Linux x86_64 with
`default-features = false`, measured on 0.1.28: ~15 MB with cargo's untuned
default release profile → 8.7 MB with the profile above → 6.0 MB with the
`release-small` profile (`opt-level = "z"`, `lto = "fat"`) → **3.4 MB** with a
single display backend plus `dist-min`. The full breakdown and the roadmap
toward smaller binaries live in [`docs/binary_size.md`](docs/binary_size.md).

## Testing

Unit and integration tests run with `cargo test`. On top of them the repo drives
**real windows**: the robot harness in `cranpose-testing` launches an app, finds
elements through the semantics tree, sends input, and captures presented frames
— so scrolling, gestures, glass rendering and frame pacing are tested as the
compositor actually presents them, not as the scene graph describes them.

```bash
./run_robot_test.sh --sequential     # the end-to-end suite
./liquid_cheatsheets.sh              # glass component reference sheets
```

See [`docs/ROBOT_TESTING.md`](docs/ROBOT_TESTING.md).

## Verification gates

```bash
cargo fmt --check
cargo test
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace --no-default-features
cargo xtask dependency-budget --strict --explain
cargo xtask binary-size --manifest-path apps/isolated-demo/Cargo.toml \
  --package isolated-demo --bin isolated-demo \
  --profile release-small --patch-workspace-cranpose --max-bytes 15728640
```

Zero warnings is the standard, not a target. Contributor conventions live in
[`AGENTS.md`](AGENTS.md); the starter project's own checks are in
[`apps/isolated-demo/README.md`](apps/isolated-demo/README.md).

## License

Apache License 2.0. See [`LICENSE-APACHE`](LICENSE-APACHE).
