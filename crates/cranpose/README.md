# Cranpose

Cranpose is a declarative UI framework for Rust. It is the primary entry point for building applications using the Cranpose system, re-exporting necessary types and macros from core, UI, and foundation crates.

## When to Use

Use this crate when you are building an end-user application. It provides the `AppLauncher` for bootstrapping the runtime and the `prelude` module which contains the most commonly used widgets (`Column`, `Row`, `Text`) and modifiers.

If you are developing a custom widget library or a low-level extension, you might prefer depending on `cranpose-core` or `cranpose-ui` directly to reduce compile times or dependency footprint.

## Key Concepts

-   **AppLauncher**: The entry point that initializes the platform-specific window (via `winit`, Android Activity, or HTML Canvas) and starts the composition loop.
-   **Prelude**: A convenience module that brings `Composer`, `Modifier`, `Element`, and core widgets into scope.
-   **Feature Flags**: Controls which platform backends (`desktop`, `android`, `web`) and renderers (`wgpu`, `pixels`) are compiled.

## Feature Flags

-   `desktop` (default): Application shell for Linux, macOS, and Windows.
-   `android`: Bindings for Android Activity.
-   `web`: Bindings for WASM/WebGL2.
-   `renderer-wgpu` (default): Hardware-accelerated rendering using `wgpu`.
-   `renderer-pixels`: Software rendering fallback using `pixels`.

## Android Host Window Sizing

Android apps can opt into best-effort primary host-window sizing with
`rememberAndroidHostWindowState(width, height)`. The requested size is expressed
in logical pixels and is separate from content layout; the actual size is updated
only from Android surface resize events.

Behavior by Android windowing mode:

-   Fullscreen activities usually keep the display-sized system bounds and
    report `AndroidHostWindowSizeStatus::Unsupported`.
-   Split-screen activities are system-managed and may clamp or ignore app
    requests.
-   Freeform and desktop-windowing activities can honor `Window.setLayout`, then
    Cranpose reconfigures WGPU and the viewport from the following resize event.
-   Overlay windows have a separate Android surface and permission model; when
    overlay mode is active, the same state resizes that surface through
    `WindowManager.updateViewLayout`.

## Android Overlay Windows

Apps that need a true always-on-top Android surface can opt into Cranpose's
overlay backend with `AppLauncher::with_android_overlay_window(...)`. The
overlay renders the app root into a Java `SurfaceView` attached through
`WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY`; pointer events from that
surface are translated into the same Cranpose input path as activity touches.

Android overlay requirements:

-   Declare `android.permission.SYSTEM_ALERT_WINDOW` in the host manifest.
-   Ask the user for overlay permission before launch; Cranpose falls back to
    the activity surface when Android denies or cannot create the overlay.
-   Include `crates/cranpose/android/java` in the Android source set so the
    `dev.cranpose.android.CranposeOverlayWindow` helper is packaged with the
    app.
-   Use Android 8.0/API 26 or newer for `TYPE_APPLICATION_OVERLAY`.
-   Treat always-on-top overlays as a product and Play policy risk; Android may
    deny, revoke, or restrict the permission outside Cranpose's control.

The overlay surface has its own lifecycle: `SurfaceView` creation, resize, touch,
and destroy callbacks are queued into the Rust Android event loop, and Cranpose
keeps the `ANativeWindow` reference alive for as long as WGPU uses that surface.
Apps can resize the active overlay with `rememberAndroidHostWindowState`; the
runtime forwards accepted size requests to `WindowManager.updateViewLayout` and
reconfigures WGPU from the following `SurfaceView` resize callback.

## Architecture

Cranpose is composed of several crates:

-   `cranpose-core`: The composition runtime, Slot Table V2, and state snapshot system. Slot Table V2 is the active runtime; gap-table material is historical rationale only.
-   `cranpose-ui`: UI primitives, layout protocol, and high-level widgets.
-   `cranpose-foundation`: Essential building blocks (Box, Row, Column) and the Modifier system.
-   `cranpose-animation`: Physics-based animation system.

## Example

```rust
use cranpose::prelude::*;

#[composable]
fn CounterApp() {
    let count = useState(|| 0);

    Column(Modifier.fill_max_size().padding(20.0), || {
        Text(format!("Count: {}", count.value()));
        
        Button(
            Modifier::empty(),
            ButtonSpec::default(),
            move || count.set(count.value() + 1),
            || Text("Increment")
        );
    });
}

fn main() {
    AppLauncher::new()
        .with_title("Counter Demo")
        .run(CounterApp);
}
```
