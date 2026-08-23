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

## Android Gradle Plugin

Cranpose's Android build lives entirely inside this crate, under `android/`:
the framework's Java, its manifest contributions, and the `dev.cranpose.android`
Gradle plugin that wires all of it into a consuming application. None of it is
published to Maven — a consuming application's `settings.gradle.kts` locates
the `cranpose` crate source that Cargo already resolved (a workspace path, a
git checkout, or the crates.io registry cache) and includes the plugin
straight from there, so `plugins { id("dev.cranpose.android") }` needs no
version.

Copy this block verbatim into `settings.gradle.kts` — it is identical for
every Cranpose Android application, in this repository or outside it:

```kotlin
pluginManagement {
    val cranposePackage = (groovy.json.JsonSlurper().parseText(
        providers.exec { commandLine("cargo", "metadata", "--format-version=1") }
            .standardOutput.asText.get()
    ) as Map<*, *>)["packages"].let { it as List<*> }
        .map { it as Map<*, *> }
        .firstOrNull { it["name"] == "cranpose" }
        ?: error("cargo metadata reports no `cranpose` package; add it as a dependency first")
    val cranposeDir = java.io.File(cranposePackage["manifest_path"] as String).parentFile
    includeBuild(cranposeDir.resolve("android/cranpose-gradle-plugin"))

    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}
```

Then in the application module's `build.gradle.kts`:

```kotlin
plugins {
    id("com.android.application")
    id("dev.cranpose.android")
}

cranpose {
    cargoPackage.set("my-app-platform")
    services.add("notifications")
}

android {
    namespace = "com.example.myapp"
    compileSdk = 36
    defaultConfig {
        applicationId = "com.example.myapp"
        minSdk = 24
        targetSdk = 36
        versionCode = 1
        versionName = "1.0"
    }
}
```

The application's own `AndroidManifest.xml` declares only what is specific to
it. There is no activity to declare, no `android.app.lib_name` to keep in
sync, no `cargo ndk` invocation to copy, and no source set pointing into the
framework's tree.

### What the plugin contributes

Every application gets `CranposeActivity` and the rest of the framework's
Java, the activity declaration with its launcher entry and
`android.app.lib_name` metadata, the provider that serves shared files, the
`androidx.appcompat` dependency it needs, and the consumer ProGuard rules that
keep the JNI surface. `cranpose { services.add(...) }` adds more, one
permission set at a time so an application that does not use a service never
asks the user for it:

| Service | What it adds |
| --- | --- |
| `background` | The foreground service Cranpose runs while a background-work lease is held, and the permissions to start it. |
| `billing` | `CranposeBilling`, the Google Play Billing library, and the permission. |
| `camera` | The camera permission and the optional camera hardware feature. |
| `haptics` | The vibrator the haptics service drives. |
| `media` | The media-playback foreground service and its permissions. |
| `notifications` | Notification posting. |
| `overlay` | Windows drawn above other applications. |
| `update` | The permission `PackageInstaller` requires to install an application update. |

### The escape hatch

Everything above is additive, not exclusive. An application adds its own
manifest entries in its own `src/main/AndroidManifest.xml` — AGP merges it
with the framework's contributions, the same as it always has. It adds its own
Gradle dependencies in its own `dependencies { }` block, and its own build
configuration in its own `android { }` block, alongside (not instead of) what
the plugin sets up. Nothing about using the plugin requires giving up direct
control of the Android build; `cranpose { }` only ever adds to it.

### Defaults the plugin applies

- Cargo features `android,renderer-wgpu` with `--no-default-features`.
- Debug builds one `x86_64` ABI, which is the emulator.
- Release builds `arm64-v8a` locally and all four ABIs on continuous
  integration, detected from `CI` / `GITHUB_ACTIONS`.
- The `release` Cargo profile, the one profile Cargo defines for every project.
  A plugin that picked anything else would be naming a profile the application
  has to declare in its own `Cargo.toml`, and a release build that stops at
  `profile is not defined` before reaching the application's code is not a
  default. An application that keeps a faster local release profile sets
  `releaseProfile` and declares the profile itself; a profile other than
  `release` keeps its debug symbols in the APK, because a profile chosen over
  `release` exists to be profiled or crash-reported on a real device.
- The Cargo build always runs and lets Cargo decide what changed, while still
  declaring its output directory — without that declaration the packaging tasks
  read a pre-Cargo snapshot and the APK silently ships the previous build.
- The native library links against the application's own `minSdk`. `cargo-ndk`
  otherwise picks API 21, whose sysroot has no `libaaudio.so`, so an app that
  enables Cranpose's audio backend fails to link over an API level its build
  never mentioned. Override with `androidApiLevel`.
- ABI directories no build is about to rewrite are removed first, so switching
  ABIs cannot leave a previous run's library to be packaged alongside the new
  one.
- Packaging is constrained to the architectures the native build produces. An
  application that ships one APK per architecture enables `splits { abi }` and
  states nothing more: the plugin writes `releaseAbis` into the split, which is
  the only way a split cannot name an architecture nothing was built for.
- Architectures normally share one Cargo pass. `debugAbiFeatures` and
  `releaseAbiFeatures` add features to individual architectures — for a native
  dependency with no port to one of them — and the plugin then runs one pass
  per distinct feature set rather than dropping the architecture or the
  feature.

Override any of them in the `cranpose { }` block.

## Android Host Window Sizing

Android apps can opt into best-effort primary host-window sizing with
`rememberAndroidHostWindowState(width, height)`. The requested size is expressed
in logical pixels and is separate from content layout; the actual size is updated
only from Android surface resize events.

Behavior by Android windowing mode:

-   Fullscreen activities usually keep the display-sized system bounds and
    report `AndroidHostWindowSizeStatus::Unsupported`. The launcher's
    `with_size` initial size is not even dispatched to a fullscreen activity:
    the window already spans the whole display edge-to-edge (including behind
    the system bars), and devices that honor `Window.setLayout` there would
    shrink the native surface and leave black bands of uncovered display.
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
    let count = rememberMutableStateOf(|| 0);

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
