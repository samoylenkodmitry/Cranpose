# Cranpose


[Cranpose Studio on JetBrains Marketplace](https://plugins.jetbrains.com/plugin/34594-cranpose) ·
[IntelliJ IDEA and RustRover guide](docs/intellij.md): live editing, component
previews, layout inspection and local platform builds.

<img width="1536" height="1024" alt="Cranpose" src="https://github.com/user-attachments/assets/2ce48dfe-a048-4b9d-8812-a0e4534691f8" />

Cranpose is a declarative UI framework for Rust, modelled on Jetpack Compose:
`#[composable]` functions, a slot-table runtime with fine-grained recomposition,
snapshot state, and a modifier-chain layout system. One Rust codebase targets
**desktop** (Linux, macOS, Windows), **Android** (including Wear OS),
**iOS**, and the **web** through WebAssembly, rendering through wgpu on all of
them.

**[Read the Cranpose Guide](https://samoylenkodmitry.github.io/Cranpose/?tab=guide)** ·
[**Explore Showcase Cranpose**](https://samoylenkodmitry.github.io/cranpose-showcase/) ·
[Releases](https://github.com/samoylenkodmitry/Cranpose/releases) ·
[crates.io](https://crates.io/crates/cranpose)


https://github.com/user-attachments/assets/13619b0f-fe49-4d0c-94a9-94a29363c2e6


https://github.com/user-attachments/assets/4cf520b8-e293-4142-8387-cfae42dee87c



## Showcase Cranpose


https://github.com/user-attachments/assets/aa9a47cf-0870-454d-8d91-43fcc7c2897c


[Showcase Cranpose](https://github.com/samoylenkodmitry/cranpose-showcase) is a polished,
cross-platform app built with Cranpose. Its live [web demo](https://samoylenkodmitry.github.io/cranpose-showcase/)
demonstrates liquid-glass surfaces, adaptive layouts, animation, and native Android and iOS builds.

## Quick start

Start from [Showcase Cranpose](https://github.com/samoylenkodmitry/cranpose-showcase),
the ready-to-run project template with desktop, Android, iOS, and web shells.
Create a repository from its GitHub template, or clone it locally and replace
the demo screens with your app. Install a [plugin](https://plugins.jetbrains.com/plugin/34594-cranpose) in RustRover or IntelliJ IDEA.

```bash
git clone https://github.com/samoylenkodmitry/cranpose-showcase.git my-cranpose-app
cd my-cranpose-app
cargo run --features desktop,renderer-wgpu
```

Or add the framework to an existing project:

```toml
[dependencies]
cranpose = { version = "0.9", features = ["desktop", "renderer-wgpu"] }
```

## Example

For an existing Kotlin or UIKit app, see the [native embedding demos](apps/native-demo/README.md).
They show a Cranpose component inside native UI and a native WebView inside Cranpose,
with state and events flowing in both directions.

State, layout, and input, in the shape the framework actually has: composables
take a `Modifier`, a spec, and their content; state comes from `rememberMutableStateOf` and is
read with `.get()` or borrowed with `.with(...)`.

```rust
use cranpose::prelude::*;

#[derive(Clone, PartialEq)]
struct Todo {
    text: String,
    done: bool,
}

#[composable]
fn TodoApp() {
    let todos = rememberMutableStateOf(|| {
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

            for index in 0..todos.with(Vec::len) {
                Row(
                    Modifier::empty().fill_max_width().clickable(move |_| {
                        todos.update(|items| items[index].done = !items[index].done);
                    }),
                    RowSpec::default().horizontal_arrangement(LinearArrangement::spaced_by(8.0)),
                    move || {
                        todos.with(|items| {
                            let todo = &items[index];
                            Text(
                                if todo.done { "[x]" } else { "[ ]" },
                                Modifier::empty(),
                                TextStyle::default(),
                            );
                            Text(todo.text.clone(), Modifier::empty(), TextStyle::default());
                        });
                    },
                );
            }

            Button(
                Modifier::empty().padding(10.0),
                ButtonSpec::default(),
                move || {
                    todos.update(|items| {
                        let position = items.len() + 1;
                        items.push(Todo { text: format!("Item {position}"), done: false });
                    });
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
`rememberLazyListState()` from `cranpose_foundation::lazy` instead of the
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
| `coroflow` | Kotlin-style coroutines and Flow, independent of any async runtime ([plan](docs/coroflow_plan.md)) |
| `cranpose-coroflow` | Coroflow inside compositions: view models and their stores, `collectAsState`, lifecycle effects |
| `cranpose-navigation` | `NavHost`, a typed back stack and a view model store per screen ([plan](docs/navigation_plan.md)) |
| `cranpose-audio` | Real-time audio (AAudio on Android/Wear OS, cpal on desktop) |
| `cranpose-media` | In-process media playback backing `cranpose_services::media` (symphonia, on the audio engine's device) |
| `cranpose-storekit` | StoreKit 2 in-app purchases (iOS/macOS) |
| `cranpose-testing` | The robot harness that drives real windows in tests |

The composition runtime uses a slot table: active groups live in preorder
group, payload, and node tables, and inactive retained branches are explicit
detached subtrees. The invariants it must uphold are documented in
[`docs/slot_table_invariants.md`](docs/slot_table_invariants.md); the design
history behind the current architecture is
[`docs/cranpose_slot_table_v2_design.md`](docs/cranpose_slot_table_v2_design.md)
(historical).

## Platform support

| Platform | Backend | Status |
|---|---|---|
| Linux x86_64 | Vulkan (GLES fallback opt-in) via wgpu | Supported; the GPU end-to-end suite runs here |
| macOS aarch64 | Metal via wgpu | Supported; builds, tests and `.app` bundles run in CI |
| Windows x86_64 | DX12/Vulkan via wgpu | Cross-built and released; not continuously exercised |
| Android / Wear OS | Vulkan/GLES via wgpu | Release APK build is checked in CI |
| iOS | UIKit/CAMetalLayer via `winit-uikit` | Simulator and device builds are checked in CI |
| Web (WASM) | WebGL2 (WebGPU opt-in via `?backend=webgpu`) | Demo build and Pages deploy are checked in CI |
| Inside another program (IntelliJ-platform IDEs) | Off-screen wgpu in a child process, streamed to the host (`embed` feature) | Experimental; [plugin template](https://github.com/samoylenkodmitry/cranpose-intellij-plugin-template) |

Release binaries for the desktop platforms are attached to each
[release](https://github.com/samoylenkodmitry/Cranpose/releases).

## Building

The commands below run from the `cranpose-showcase` checkout created in **Quick
start**. The [Cranpose Guide](docs/guide.md#get-started) also covers a project built
from an empty Cargo package.

### Desktop (Linux/macOS/Windows)

Use a computer with the target desktop OS and a GPU driver for Vulkan, Metal or
DirectX 12.

```bash
cargo run --features desktop,renderer-wgpu
```

To package the Cranpose demo as a macOS `.app`, run the workspace task from the
Cranpose repository root:

```bash
cargo xtask bundle-macos \
  --package desktop-app \
  --bin desktop-app \
  --app-name "Cranpose Demo" \
  --bundle-id io.cranpose.demo
```

Pass `--resources <dir>` to copy resources into `Contents/Resources`, and
`--sign-identity <id>` to run the explicit codesign step.

On Windows, a release build opens no terminal window when `main.rs` starts
with the attribute the demos carry:

```rust
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
```

The attribute belongs to the binary, so the framework cannot set it for an
application; a debug build keeps its terminal for the log. Helper programs the
framework runs, such as the registry reads for the theme, start without a
window of their own, and `cranpose_services::windowless_command` starts an
application's own the same way.

### Android

Use Windows, Linux or macOS with the Android SDK, NDK and the project's JDK.
Build an APK for an ARM Android device from the starter checkout:

```bash
cd android
./gradlew :app:assembleDebug -PshowcaseAbi=arm64-v8a
```

### iOS

Use a Mac with Xcode and an iOS simulator runtime. From the starter checkout, run
the Apple Silicon simulator build:

```bash
rustup target add aarch64-apple-ios-sim aarch64-apple-ios
./ios/run-sim.sh
```

For Intel Macs, use the `x86_64-apple-ios` target through the manual steps in the
[iOS guide](docs/guide.md#run-on-ios). Device builds need an iPhone or iPad,
a development certificate and a provisioning profile. Developers
on Windows or Linux can use a GitHub macOS runner; the guide includes a
[workflow example](docs/guide.md#build-apple-targets-with-github-actions).

### Web (WASM)

Build on Windows, Linux or macOS. Use Git Bash for the shell script on Windows.
Run the starter in a browser with WebGPU support and a compatible GPU driver:

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-pack
./build-web.sh
cd dist
python3 -m http.server 8080
```

Open `http://localhost:8080`. Use HTTPS for a public web host.

### Inside an IDE (IntelliJ plugin)

The `embed` feature runs an app inside another program's window: frames are
drawn off screen and streamed to the host, which sends input, theme and
messages back. The
[IntelliJ plugin template](https://github.com/samoylenkodmitry/cranpose-intellij-plugin-template)
uses it to write tool windows in Rust with Cranpose, shaders included.

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
  face that text draws in when the app supplies no fonts. An app that
  supplies fonts through any `AppLauncher` font method (`with_fonts`,
  `with_font_family`, system or asset fonts) leaves it out of its binary
  without touching the feature; the launcher's type becomes
  `AppLauncher<AppFonts>`.
- `renderer-wgpu-gles` (off by default): the GL/GLES fallback for desktop
  machines without a working Vulkan driver. Leaving it off removes the GLES
  half of wgpu and naga's GLSL writer. Android always compiles the GLES
  fallback; web always compiles WebGL.
- `desktop-x11` / `desktop-wayland`: `desktop` compiles both display-server
  backends; picking one drops the other's window and input stack.

```toml
[dependencies]
cranpose = { version = "0.9", default-features = false, features = [
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
elements through the semantics tree, sends input, and captures presented frames.

```bash
just robot         # the end-to-end suite
just cheatsheets   # glass component reference sheets
just perf-max-fps  # unthrottled frame rate on the heaviest glass scene
```

See [`docs/ROBOT_TESTING.md`](docs/ROBOT_TESTING.md).

## Verification gates

```bash
just ci
```

That is formatting, spell check, version alignment, the test suite, clippy,
rustdoc, and the architecture budgets: featureless and all-features builds, the
per-backend winit checks, the duplicate-dependency budgets and the desktop
binary-size ceiling.

Zero warnings is the standard. Contributor conventions live in
[`AGENTS.md`](AGENTS.md).

## License

Apache License 2.0. See [`LICENSE`](LICENSE).
