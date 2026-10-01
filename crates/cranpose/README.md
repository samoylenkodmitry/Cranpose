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
    compileSdk = 37
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
`android.app.lib_name` metadata, the provider that serves shared files, and
the consumer ProGuard rules that keep the JNI surface. It gets no library
dependency and no permission: the framework declares none, not even
`INTERNET`. `cranpose { services.add(...) }` adds the code and components
a service needs, and the application declares that service's permissions in
its own manifest:

| Service | What it adds | Permissions the application declares |
| --- | --- | --- |
| `background` | The foreground service Cranpose runs while a background-work lease is held. | `FOREGROUND_SERVICE`, `FOREGROUND_SERVICE_DATA_SYNC` |
| `billing` | `CranposeBilling` and the Google Play Billing library. | `com.android.vending.BILLING` |
| `camera` | The camera capability. | `CAMERA` |
| `haptics` | The vibrator the haptics service drives. | `VIBRATE` |
| `media` | The media-playback foreground service. | `FOREGROUND_SERVICE`, `FOREGROUND_SERVICE_MEDIA_PLAYBACK` |
| `network` | `cranpose_services::http` and the online / metered state the activity reports. | `INTERNET`, `ACCESS_NETWORK_STATE` |
| `notifications` | Notification posting. | `POST_NOTIFICATIONS` |
| `overlay` | Windows drawn above other applications. | `SYSTEM_ALERT_WINDOW` |

### One declaration, in Rust, for every platform

A permission is a line in the store listing and a question to the person
holding the phone. It belongs to the application, and an application that
ships on five platforms should say it once rather than in an Android manifest,
an `Info.plist` and a Gradle block that can drift apart.

So it says it in its build script:

```rust
use cranpose::capabilities::{Demand, Use, declare};

fn main() {
    declare(&[
        Use::camera("Reads a receipt with the camera. Nothing leaves this device."),
        Use::notifications(),
    ])
    .emit();
}
```

A service is a function, so a name cannot be misspelled. A service Apple shows
a sentence for takes that sentence as an argument, so it cannot be forgotten.
Hardware an application cannot run without is an enum: `.demanding(&[Demand::Watch])`.

`emit` writes the Android permissions and feature declarations, the Apple
usage descriptions, and a constant the application itself reads:

```rust
cranpose::app_capabilities!();

AppLauncher::new().with_capabilities(&CAPABILITIES)
```

The Android build takes the permissions from there. An application that has
not declared anything in Rust keeps working: the build then reads
`cranpose { services }` as before, and refuses a service whose permission the
application's own manifest does not hold.

Installing an update has no entry in `cranpose { services }`: `Use::update()`
is its one declaration. From it the Android build writes
`REQUEST_INSTALL_PACKAGES`, compiles `CranposeAppUpdate` from
`android/java-update`, and declares that class as a receiver with
`android:exported="false"`; the framework registers the Android updater only
when the `CAPABILITIES` the launcher receives hold the service. An application
that does not declare it carries none of this code, and no other application
can send an intent to the installer's receiver.

### Hardware features stay optional unless you ask

A permission carries hardware with it. `android.permission.CAMERA` makes
Android's packaging tools require `android.hardware.camera`, which takes the
application off every device without a camera and stops updates for people who
already have it installed — a loss Play reports only once a release is
prepared, in a warning that is easy to read past.

The plugin writes those features into the merged manifest as
`android:required="false"`, one for each permission that carries one, and
refuses a build where a feature stays required that the application did not
ask for. An application that cannot work without the hardware names it:

```kotlin
cranpose {
    services.add("camera")
    requiredFeatures.add("android.hardware.camera")
}
```

A watch-only application says it in Rust instead, with
`.demanding(&[Demand::Watch])`, and the build writes
`android.hardware.type.watch` into the manifest as required. An application
that only reads a photo it was given says nothing and reaches every device.
The refusal names the feature, the permission behind it, and the line that
would allow it.

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
- Release builds `arm64-v8a`. Override with `releaseAbis` or the
  `cranposeReleaseAbis` Gradle property.
- The `release` Cargo profile, the one profile Cargo defines for every project.
  A plugin that picked anything else would be naming a profile the application
  has to declare in its own `Cargo.toml`, and a release build that stops at
  `profile is not defined` before reaching the application's code is not a
  default. An application that keeps a faster local release profile sets
  `releaseProfile` and declares the profile itself; a profile other than
  `release` keeps its debug symbols in the APK, because a profile chosen over
  `release` exists to be profiled or crash-reported on a real device.
- The Android build type selects the native settings. `debug` uses
  `debugProfile`, `debugAbis` and `debugAbiFeatures`; `release` uses their release
  counterparts, including when the APK is debuggable. Custom build types use
  debug settings when debuggable and release settings otherwise. Flavor names
  do not affect this selection.
- Each variant has one `cranposeBuildNative<Variant>` task. Its generated JNI
  libraries and capability declarations belong to that variant under the Android
  module's `build/generated/cranpose/<variant>/` directory. Packaging, manifest
  checks and declared Java sources depend on the same producer.
- Cargo builds share their normal build cache. Native producers run one at a
  time within a Gradle build, including the cargo-ndk export step, so one
  variant cannot replace another's library while it is being copied. Cargo
  still decides what needs recompiling on every invocation.
- The native library links against the lowest effective `minSdk` among enabled
  application variants, including product flavors. `cargo-ndk`
  otherwise picks API 21, whose sysroot has no `libaaudio.so`, so an app that
  enables Cranpose's audio backend fails to link over an API level its build
  never mentioned. Override with `androidApiLevel`.
- Each native build clears its own generated JNI output before export, so
  switching ABIs or library names cannot package an earlier library.
- Packaging is constrained to the architectures the native build produces. An
  application that ships one APK per architecture enables `splits { abi }` and
  states nothing more: the plugin enables only the split outputs matching
  that variant's selected ABIs.
- `debugAbiFeatures` and `releaseAbiFeatures` add features to individual
  architectures. Each ABI builds with its selected features and contributes its
  declared permissions, hardware requirements and services to the APK.
  ABI split APKs share this variant manifest and service Java, so the combined
  requirements apply to every split.
- Declaration writers use stable directories for each native configuration.
  Equivalent flavors reuse Cargo's cached build script results; each variant
  receives its own declaration copies before manifest and Java processing.

Override build settings in the `cranpose { }` block. The plugin owns
`CRANPOSE_CAPABILITIES_DIR`; other Cargo environment variables can be set through
`environment`.

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

-   `cranpose-core`: The composition runtime, the slot table, and the state snapshot system; gap-table material is historical rationale only.
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
