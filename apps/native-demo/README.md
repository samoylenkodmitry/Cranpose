# Native mobile embedding

These are ordinary Kotlin/Android and Swift/UIKit applications. Each app has two modes:

| Mode | Demonstration |
| --- | --- |
| Native → Cranpose | Native controls surround a Cranpose component. Native and Rust buttons update the same Rust state. |
| Cranpose → Native | Cranpose lays out a real Android WebView or WKWebView. Rust supplies its HTML and bounds; tapping its link updates Rust state. Rust can remove and remount the native view. |

The WebView page is bundled and needs neither network access nor an API key. Switching modes creates a fresh component. Android uses an Activity with a FrameLayout; iOS uses a UIViewController with a UIView. Neither demo starts the standalone Cranpose mobile application loop.

## Build and run

Run commands from the repository root. Use the repository Rust toolchain and `just`.
Bindings are generated automatically with UniFFI from the shared Rust demo library.
Generated Swift, Kotlin, headers and native libraries stay under `apps/native-demo/generated/` and are not committed.

### Android

Install JDK 17 or newer, Android SDK platform 37, the NDK and `cargo-ndk`.
Set `ANDROID_HOME` to the SDK and add its platform-tools to `PATH`.
The existing repository Gradle wrapper supplies Gradle.

```sh
rustup target add aarch64-linux-android
just android-native-demo
adb -s DEVICE_SERIAL install -r apps/native-demo/android/app/build/outputs/apk/release/app-release.apk
adb -s DEVICE_SERIAL shell am start -n dev.cranpose.nativehost/.MainActivity
```

The APK uses the debug signing key for local installation. The default Rust profile is `dev`; use `PROFILE=release just android-native-demo` for optimized Rust.
For an x86_64 emulator, install the `x86_64-linux-android` Rust target and set `NATIVE_DEMO_ABI=x86_64` when building.
After generating bindings and libraries, the `apps/native-demo/android` directory can also be opened in Android Studio.

### iOS

On an Apple Silicon Mac with Xcode and its command-line tools:

```sh
rustup target add aarch64-apple-ios-sim
just ios-native-demo
xcrun simctl install booted target/aarch64-apple-ios-sim/debug/CranposeNativeDemo.app
xcrun simctl launch booted dev.cranpose.nativehost
```

The script compiles the UIKit sources and generated Swift bindings, links the Rust static library, and signs the simulator app.
`PROFILE=release just ios-native-demo` writes an optimized bundle under `target/aarch64-apple-ios-sim/release/`.
`CARGO_TARGET_DIR` changes these output roots.

For a physical device, install `aarch64-apple-ios` and run `just ios-native-demo aarch64-apple-ios`. Device installation additionally needs your Apple development signing identity and provisioning profile for `dev.cranpose.nativehost`; the default ad hoc simulator signature is insufficient. The demo does not configure a development team or provision a device.

## Framework APIs

`AppLauncher::create_embedded_view(width, height, density, content)` creates an independent `EmbeddedView` with no window or event loop. Dimensions are physical pixels; layout, native slots and pointer coordinates use logical points. Configure fonts on the launcher as usual.

```rust
use cranpose::{AppLauncher, native_view::{NativeView, NativeViewHost}, prelude::*};

let native = NativeViewHost::default();
let content_host = native.clone();
let mut component = AppLauncher::new().create_embedded_view(720, 960, 2.0, move || {
    NativeView(
        content_host.clone(),
        "web",
        "<html><body>Hello from Rust</body></html>",
        Modifier::empty().fill_max_width().height(210.0),
        |event| println!("Native event: {event}"),
    );
})?;
let mut pixels = Vec::new();
if component.draw(&mut pixels)? {
    let children = native.layout(component.shell().layout_tree());
    println!("Present {} bytes with {} native children", pixels.len(), children.len());
}
Ok::<(), cranpose::embedded_view::EmbeddedViewError>(())
```

Enable Cranpose's `renderer-wgpu` feature for `EmbeddedView`. `NativeView` itself is renderer-independent.

The launcher configures embedded fonts. Platform services and application lifecycle belong to the native host; these adapters do not initialize the standalone Android/iOS service backends.

Keep each component on its owning thread. Route input through `component.shell()`, call `resize` after native layout/density changes, and call `set_visible` when detached or hidden. Install `set_frame_waker` and honor `frame_schedule` so idle components do not poll. `draw` returns false without reading back pixels when the view is unchanged or hidden.

`NativeViewHost` owns the application's native factory requests. `NativeView` reserves a sized layout slot; it does not invoke platform APIs on the Rust thread. The platform creates a view for `kind`, updates it from `value`, and routes its events through `dispatch(id, event)`. Keep the host stable, preserve native instances while their IDs remain mounted, and dispose missing IDs. If a slot's kind changes, recreate its native instance. A map adapter can use the same mechanism with an application-defined `map` factory and configuration.

## Adapter architecture and limits

The demo's UniFFI bridge owns the composition and GPU on one Rust worker thread. Native hosts coalesce frame requests, await rendering off the UI thread, and update UIKit/Android views on the UI thread. The GPU target and readback buffer are reused; unchanged frames carry no pixels. The native WebView receives touch, focus and accessibility directly.

The portable `EmbeddedView` adapter reads GPU pixels into native images. It is appropriate for demonstrating integration, but incurs a full image transfer for changed frames; it is not a zero-copy Metal/SurfaceView integration. A Rust GPU host can use `AppShell<WgpuRenderer>` directly with its own target.

Native children are axis-aligned overlays above Cranpose drawing, clipped to the host container. Arbitrary transforms, ancestor clipping, interleaved z-order and Cranpose effects over a native child are not supported. Hide native children when presenting overlapping Cranpose popups. These example hosts route one touch pointer; they do not implement a native text-input or accessibility bridge for the Rust-rendered content. Native WebView accessibility remains available.

## Tests

```sh
just test-native-demo
just test-native-demo-android DEVICE_SERIAL
just test-native-demo-ios SIMULATOR_UDID
```

Build the corresponding app first. Android tests install a debug APK on the selected device. iOS tests reuse the repository's LiquidReference XCTest runner against the separately installed demo.

The Rust tests cover slot bounds and stable identity, state updates, disposal, touch routing, resizing and density changes, invalid dimensions, hidden views and idle-frame suppression. Native UI tests exercise native → Rust, Rust → native host, and WebView → Rust events, plus native child removal and remounting.
