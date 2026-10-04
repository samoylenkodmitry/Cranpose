# Cranpose

Build desktop, mobile and web apps with Rust functions. Cranpose follows the
Jetpack Compose model: a function describes the screen, state changes update the
screen, and modifiers control layout, input and appearance.

The same UI code runs on Linux, macOS, Windows, Android, Wear OS, iOS and the web.
The experimental watchOS host uses the software renderer. Cranpose also places
Rust screens inside Android Compose and UIKit apps.

## Run a counter

Create a desktop app:

```sh
cargo new counter
cd counter
cargo add cranpose --features desktop,renderer-wgpu
```

Replace `src/main.rs` with this example, then run `cargo run`:

```rust,no_run
use cranpose::*;

#[composable]
fn Counter() {
    let count = rememberMutableStateOf(|| 0);

    Column(
        Modifier::empty().padding(24.0),
        ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(12.0)),
        move || {
            Text(
                format!("Count: {}", count.get()),
                Modifier::empty(),
                TextStyle::default(),
            );
            Button(
                Modifier::empty().height(48.0),
                ButtonSpec::default(),
                move || count.update(|value| *value += 1),
                || {
                    Text("Increment", Modifier::empty(), TextStyle::default());
                },
            );
        },
    );
}

# #[cfg(all(feature = "desktop-shell", feature = "renderer-wgpu", not(target_os = "android")))]
fn main() -> Result<(), cranpose::LaunchError> {
    AppLauncher::new()
        .with_title("Counter")
        .with_size(360, 240)
        .try_run(Counter)
}
# #[cfg(not(all(feature = "desktop-shell", feature = "renderer-wgpu", not(target_os = "android"))))]
# fn main() {}
```

`rememberMutableStateOf` retains the count across recomposition. The button
callback changes the count. A read through `count.get()` subscribes the text to
state updates. State handles are `Copy`, so the callbacks share one state value.
`try_run` starts the desktop event loop and returns a launch error on failure.

## Choose a project structure

For a complete app, start from the
[project template](https://github.com/samoylenkodmitry/cranpose-showcase).
The template includes screens, view models, navigation and platform hosts.
The [IDE plugin](https://plugins.jetbrains.com/plugin/34594-cranpose) creates the
same project through **File → New Project → Cranpose**.

Use `cranpose::prelude::*` for common app imports and `cranpose::liquid::prelude::*`
for Liquid controls. The [app guide](https://samoylenkodmitry.github.io/Cranpose/?tab=guide)
connects UI state, flows, view models, device services and platform builds.

## Select a platform

Select a host and renderer in Cargo features. Desktop uses Vulkan, Metal or
DirectX through WGPU. Android and web also support GL/GLES.

| Target | Features | Host setup |
| --- | --- | --- |
| Linux, macOS, Windows | `desktop`, `renderer-wgpu` | `AppLauncher::try_run` |
| Linux with one display backend | `desktop-x11` or `desktop-wayland`, plus `renderer-wgpu` | Select the session backend |
| Android and Wear OS | `android`, `renderer-wgpu` | `android_main!` and the Android Gradle plugin |
| iOS | `ios`, `renderer-wgpu` | Xcode host and iOS entry point |
| Web | `web`, `renderer-wgpu` | Wasm entry point and browser canvas |
| watchOS, experimental | `watchos` | SwiftUI host and software renderer |

The [platform guide](https://docs.rs/cranpose/latest/cranpose/_docs/platforms/index.html)
links the target-specific setup. The
[Android plugin guide](https://github.com/samoylenkodmitry/Cranpose/blob/main/crates/cranpose/android/README.md)
describes Cargo packages, services, permissions and ABI selection.

The default feature embeds a font. `default-features = false` lets an app select
its own font data. `renderer-pixels` selects software output for custom hosts.
`renderer-wgpu-gles` adds the GL/GLES backend to WGPU.

## Add app features

| Task | Start here |
| --- | --- |
| Arrange controls and text | [UI widgets](https://docs.rs/cranpose-ui/latest/cranpose_ui/) |
| Retain screen state | [State guide](https://docs.rs/cranpose/latest/cranpose/_docs/state/index.html) |
| Build a long list or custom layout | [Layout guide](https://docs.rs/cranpose/latest/cranpose/_docs/layout/index.html) |
| Add glass controls and themes | [Liquid UI](https://docs.rs/cranpose-liquid/latest/cranpose_liquid/) |
| Animate a value | [Animation](https://docs.rs/cranpose-animation/latest/cranpose_animation/) |
| Collect flows and retain view models | [Coroflow integration](https://docs.rs/cranpose-coroflow/latest/cranpose_coroflow/) |
| Navigate between screens | [Navigation](https://docs.rs/cranpose-navigation/latest/cranpose_navigation/) |
| Use files, clipboard, network or purchases | [Device services](https://docs.rs/cranpose-services/latest/cranpose_services/) |
| Declare permissions and hardware | [Capabilities](https://docs.rs/cranpose-capabilities/latest/cranpose_capabilities/) |
| Embed a Rust screen in a native app | [Native integration](https://docs.rs/cranpose-native/latest/cranpose_native/) |

Optional features select extra backends: `audio`, `audio-desktop`, `media`,
`camera-desktop`, `storekit`, `playbilling` and `webview`. `robot` adds app test
support. The
[feature list](https://docs.rs/crate/cranpose/latest/features) records each
dependency choice.

Cranpose is pre-alpha. Public APIs can change between releases. Review the
[release notes](https://github.com/samoylenkodmitry/Cranpose/releases) when an app
updates its dependency.
