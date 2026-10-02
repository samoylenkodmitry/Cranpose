# Native mobile embedding

These ordinary Kotlin/Android and Swift/UIKit applications demonstrate both directions:

| Mode | Behavior |
| --- | --- |
| Native → Cranpose | Native controls and Rust controls update one shared counter. |
| Cranpose → Native | Cranpose positions a real Android WebView or WKWebView loading https://example.com. |

The website mode opens by default. It requires internet access. The page can navigate and scroll;
counter updates preserve its native instance and loaded document. Removing and mounting the
WebView creates a fresh instance. Loading failures appear below the WebView.

## Application API

The reusable implementation lives in the framework:

- `crates/cranpose-native`: composition ownership, worker, commands, frame callbacks, events and bindings.
- `platforms/android`: the Android library containing `CranposeView`, factory lifecycle and optional `WebViewFactory`.
- `platforms/ios/Sources/Cranpose`: the equivalent UIKit module.
- `crates/cranpose`: lower-level `EmbeddedView` and renderer-independent `NativeViewHost`.
- `cranpose::WebView`: the shared website composable, also re-exported by `cranpose-native`.

The [desktop demo's WebView tab](../desktop-demo/src/app/webview.rs) uses this same
composable in regular Cranpose applications on desktop, Android, iOS and web.
Its built-in hosts require the `cranpose/webview` feature; native component hosts
continue to register `WebViewFactory` in their containing application.

The demo keeps its content and application events. It does not implement a render loop, pixel
conversion, input routing, slot reconciliation or platform WebView management.

Kotlin:

```kotlin
val component = CranposeView(
    context,
    createDemoComponent(nativeChildren = true, website = "https://example.com"),
    mapOf("web" to WebViewFactory()),
)
component.onEvent = { event ->
    if (event.name == "count") label.text = event.value
}
component.onError = { message -> label.text = message }
container.addView(component)
component.sendEvent("increment")
```

Swift:

```swift
let component = CranposeView(
    session: createDemoComponent(nativeChildren: true, website: "https://example.com"),
    factories: ["web": WebViewFactory()]
)
component.onEvent = { event in
    if event.name == "count" { label.text = event.value }
}
component.onError = { message in label.text = message }
container.addSubview(component)
component.sendEvent("increment")
```

Lay out the component like any other native view. Both adapters handle attach/detach, visibility,
application backgrounding, density, touch and display-frame scheduling. Call `close()` when
permanently discarding a component; temporary detachment preserves its state. A closed component
cannot be remounted. The Android demo closes its component from `onDestroy`.

Rust applications export a factory returning `Arc<NativeSession>`. The factory passed to
`NativeSession::new` executes on the owning worker, where non-Send composition state can be created.
Return `NativeContent::new(content).on_event(handler)` to connect application input.
`SendToHost(name, value)` delivers output after composition commits.

```rust
use cranpose::prelude::*;
use cranpose_native::{SendToHost, WebView, WebViewEvent};

WebView(
    "https://example.com",
    Modifier::empty().fill_max_width().height(300.0),
    |event| match event {
        WebViewEvent::Loaded(url) => println!("Loaded {url}"),
        WebViewEvent::Failed(message) => eprintln!("{message}"),
    },
);
SendToHost("ready", "true");
```

Use these composables within `NativeContent`; the runtime supplies their native-host context.

## Custom native views

Register a factory under the same kind on both platforms, then call
`cranpose_native::NativeView(kind, configuration, modifier, on_event)` from Rust.

Each factory implements `create` and returns a child with a native `view`, `update`,
`setVisible` and `dispose`. The framework owns sizing, event routing and lifetime:

- An existing slot keeps its instance across configuration and bounds changes.
- Changing a slot's kind disposes the old child and creates the newly selected factory.
- Unmounting a slot disposes its child. Stale events do not reach a new child.
- Unknown factories report `onError` on both platforms.
- Updates receive the latest configuration; a factory should avoid reloading unchanged content.

A map adapter uses this contract with an application-selected map SDK and credentials.
WebView is optional: register `WebViewFactory` only when it is needed.
Both website adapters accept HTTPS, enable JavaScript and local DOM storage, allow HTTPS
navigation, and report main-frame load or HTTP failures. They do not expose a JavaScript-to-Rust
object or bypass certificate checks. The Android consuming app supplies its INTERNET permission.

## Build and run

Run from the repository root with the repository Rust toolchain and `just`.

### Android

Install JDK 17+, Android SDK platform 37, the NDK and `cargo-ndk`.
Set `ANDROID_HOME` and add its platform-tools to PATH.

```sh
rustup target add aarch64-linux-android
just android-native-demo
adb -s DEVICE_SERIAL install -r apps/native-demo/android/app/build/outputs/apk/release/app-release.apk
adb -s DEVICE_SERIAL shell am start -n dev.cranpose.nativehost/.MainActivity
```

The APK uses local debug signing. Rust defaults to the dev profile; use
`PROFILE=release just android-native-demo` for optimization.
For x86_64 emulators install `x86_64-linux-android` and set `NATIVE_DEMO_ABI=x86_64`.

The demo includes the framework as Gradle project `:cranpose`.
`cranposeBindingsDir` points that module at the generated runtime Kotlin sources.
Application bindings remain in the app source set. The library carries its own coroutine and JNA dependencies.
Another application can include the same library project, generate bindings from its Rust library, and
set the UniFFI `cdylib_name` to that library.

To select another website when launching Android, append
`--es website https://YOUR_SITE`. The `--ez native-component true` option starts the other demo mode.

### iOS

On an Apple Silicon Mac with Xcode:

```sh
rustup target add aarch64-apple-ios-sim
just ios-native-demo
xcrun simctl install SIMULATOR_UDID target/aarch64-apple-ios-sim/debug/CranposeNativeDemo.app
xcrun simctl launch SIMULATOR_UDID dev.cranpose.nativehost
```

The build creates separate `CranposeBindings` and `Cranpose` Swift modules.
The first contains the generated runtime and application bindings; the second contains the reusable
UIKit hosts and imports only the runtime API. The app imports both modules.
Their static libraries and Swift modules are under `target/TARGET/PROFILE/native-modules`.
The same source module can be compiled against another application's generated `CranposeBindings`.

`PROFILE=release just ios-native-demo` creates optimized Rust.
`CARGO_TARGET_DIR` changes the output root.
Physical devices use `just ios-native-demo aarch64-apple-ios` and additionally require an Apple
signing identity and provisioning profile. The default signature supports the simulator only.

## Rendering scope

The native host APIs have the same concepts and lifecycle on Android and iOS, with idiomatic
Kotlin/Swift syntax. Shared Cranpose content uses the same renderer; surrounding native controls
retain their platform appearance. Other platforms can use the lower-level Rust primitives;
this package currently supplies Android and UIKit host views.

Presentation currently reads changed GPU images into native bitmap/image views. Idle compositions
do not read pixels or poll for frames. This is a portable implementation with full image transfers
for changed frames. The presentation details are private to the framework, so an application does
not need to change its integration when a native GPU surface backend is added.

Native children are axis-aligned overlays above Cranpose, clipped to their host container.
Ancestor clipping, arbitrary transforms, interleaved z-order and Cranpose effects over a native
child are not supported. Hide native children before displaying overlapping Cranpose popups.
The adapters route one touch pointer and do not yet bridge Rust text input or accessibility.
The native WebView retains its own input and accessibility.

## Validation

```sh
just test-native-demo
just test-native-demo-android DEVICE_SERIAL
just test-native-demo-ios SIMULATOR_UDID
```

Build the corresponding app first. Rust tests cover sessions, event isolation, visibility,
resizing, native identity, load events and shutdown. Native UI tests cover the two embedding
directions, a live website, shared state, WebView preservation, and removal/remounting.
The live website tests require internet access. Pull-request CI builds both native applications
and their framework modules; simulator UI tests use the explicit recipes above.
