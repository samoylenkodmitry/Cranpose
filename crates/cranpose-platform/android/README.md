# Cranpose Platform Android

Android platform integration layer for Cranpose.

## When to Use

This crate connects the Cranpose runtime with the Android operating system. Users do not interact with this crate directly; it is used implicitly when the `android` feature is enabled in `cranpose`. It handles lifecycle events from `android-activity` and translates touch input into Cranpose pointer events.

## Key Concepts

-   **Activity Lifecycle**: Responds to Android lifecycle events (onResume, onPause, onDestroy) to manage resource allocation.
-   **Native Window**: Interfaces with the Android `ANativeWindow` to provide a surface for the renderer.
-   **Input Translation**: Converts raw Android input events (AMotionEvent, AKeyEvent) into the unified Cranpose event system.

## Example

The standard entry point for an Android application using [`android-activity`](https://crates.io/crates/android-activity):

```rust
use cranpose::prelude::*;

#[no_mangle]
fn android_main(app: android_activity::AndroidApp) {
    AppLauncher::new()
        .run(app, MyApp);
}
```
