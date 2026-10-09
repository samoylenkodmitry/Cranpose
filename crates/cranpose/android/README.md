# Android host and Gradle plugin

The `dev.cranpose.android` plugin builds a Rust `cdylib`, copies the library into
the APK and adds Cranpose's activity, Java services and manifest entries.
The plugin ships inside the `cranpose` Cargo package. Gradle resolves the plugin
from the package directory selected by Cargo.

Use the Android SDK, NDK, JDK and `cargo-ndk` on the build host. The
[project template](https://github.com/samoylenkodmitry/cranpose-showcase/tree/main/android)
provides a complete Android host. The steps below add the plugin to a custom host.

## Resolve the plugin

Add the `cranpose` dependency to the Rust app first. In `settings.gradle.kts`,
read the resolved Cargo package directory and include the Gradle plugin:

```kotlin
pluginManagement {
    val cranposePackage = (groovy.json.JsonSlurper().parseText(
        providers.exec { commandLine("cargo", "metadata", "--format-version=1") }
            .standardOutput.asText.get()
    ) as Map<*, *>)["packages"].let { it as List<*> }
        .map { it as Map<*, *> }
        .firstOrNull { it["name"] == "cranpose" }
        ?: error("Add the cranpose Cargo dependency first")
    val cranposeDir = java.io.File(cranposePackage["manifest_path"] as String).parentFile
    includeBuild(cranposeDir.resolve("android/cranpose-gradle-plugin"))

    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}
```

This setup supports workspace paths, Git dependencies and registry packages.
The Cargo package selects the plugin version.

## Configure the app module

The Rust package needs a `cdylib` target, Android features and an
[`android_main!`](https://docs.rs/cranpose/latest/cranpose/macro.android_main.html)
entry point. The Gradle module selects the package:

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

Choose Android SDK values for the app's distribution requirements. The plugin
adds `CranposeActivity`, library metadata, ProGuard rules and service components.
The app's `AndroidManifest.xml` holds app-specific entries.

## Declare services and permissions

Use [`cranpose-capabilities`](https://docs.rs/cranpose-capabilities/latest/cranpose_capabilities/)
in `build.rs` to declare services, permission descriptions and required hardware.
`Declaration::emit` writes platform metadata and a Rust constant. The app reads
the constant through `cranpose::app_capabilities!()` and passes `CAPABILITIES`
to `AppLauncher::with_capabilities`.

The Gradle `services` set also supports manually maintained app manifests:

| Service | Platform component | Permission |
| --- | --- | --- |
| `background` | Foreground task service | `FOREGROUND_SERVICE`, `FOREGROUND_SERVICE_DATA_SYNC` |
| `billing` | Play Billing client | `com.android.vending.BILLING` |
| `camera` | Camera backend | `CAMERA` |
| `haptics` | Vibrator backend | `VIBRATE` |
| `media` | Media-session service | `FOREGROUND_SERVICE`, `FOREGROUND_SERVICE_MEDIA_PLAYBACK` |
| `microphone-standby` | Foreground service of type microphone that keeps recording possible from the background (`cranpose_services::hold_microphone_standby`) | `FOREGROUND_SERVICE`, `FOREGROUND_SERVICE_MICROPHONE`, `RECORD_AUDIO` |
| `network` | HTTP and connectivity state | `INTERNET`, `ACCESS_NETWORK_STATE` |
| `notifications` | Notification backend | `POST_NOTIFICATIONS` |
| `overlay` | Surface above other apps | `SYSTEM_ALERT_WINDOW` |
| `wearable` | Messages and streams to the app on the paired phone or watch (Wear OS Data Layer, `cranpose_services::wearable`) | none |

`Use::update()` also selects the package installer, its receiver and
`REQUEST_INSTALL_PACKAGES`. The launcher registers the updater when the declared
capabilities include updates.

Hardware features implied by permissions remain optional in the manifest.
Declare mandatory hardware through `Demand`, or add an Android feature name to
`requiredFeatures`. The plugin checks the final manifest against these choices.

## Select builds and ABIs

| Option | Default or behavior |
| --- | --- |
| `workspaceRoot` | `../../../..` relative to the Gradle project |
| `features` | `android`, `renderer-wgpu` |
| `defaultFeatures` | `false` |
| `debugProfile` / `releaseProfile` | `dev` / `release` |
| `debugAbis` | `x86_64` |
| `releaseAbis` | All four Android ABIs; `cranposeReleaseAbis` can override the set |
| `debugAbiFeatures` / `releaseAbiFeatures` | Extra Cargo features per ABI |
| `libraryName` | Cargo package name with dashes replaced by underscores |
| `androidApiLevel` | Lowest effective `minSdk` among enabled app variants |
| `environment` | Extra Cargo environment variables |

Use profiles declared by the app. Release builds strip native debug symbols
when the selected profile is `release`; custom profiles retain symbols.
The Android `debug` and `release` build types select their own
profiles, ABIs and features. Other build types use the debuggable flag.

Each variant owns a `cranposeBuildNative<Variant>` task and generated output under
`build/generated/cranpose/<variant>`. Native tasks run one at a time within a
Gradle build and share Cargo's cache. ABI splits use the selected native ABIs.
Each split receives the variant's combined service and permission declarations.
The plugin owns `CRANPOSE_CAPABILITIES_DIR`.

## Ship pipelines for fresh installs

A launch compiles the GPU pipelines its screens draw with and keeps them in a
cache file, so later launches skip the compiles. A fresh install has no such
file; on a slow GPU, its first launch shows placeholders while it compiles.

`cranposeRecordPipelines<Variant>` installs the variant and runs it for ten
seconds on every connected device. It then pulls each device's cache file
into `src/main/assets/cranpose_gpu/`:

```bash
./gradlew :app:cranposeRecordPipelinesRelease
```

The next build ships those files. A fresh install on a device with the same
GPU and driver starts from that device's compiled pipelines and launches like
a relaunch. Any other device takes the list of pipelines the app draws from
one of the files and compiles those ahead of its first screen. Record again
after changing what the first screen draws or the Cranpose version.

## Host windows and web views

`rememberAndroidHostWindowState` requests a window size in logical pixels.
Android surface events supply the actual size. Fullscreen hosts keep system
bounds; freeform and desktop modes can accept app size requests. Split-screen
bounds follow system policy.

`AppLauncher::with_android_overlay_window` selects an overlay surface on Android
8/API 26 or later. Declare the `overlay` service and request the overlay
permission before launch. Permission refusal selects the activity surface.

The `webview` Cargo feature adds `WebView`. Declare network access for remote
pages. Native browser views occupy axis-aligned bounds above Cranpose content
and own their input and accessibility. Dismiss a browser view before a Cranpose
popup needs the same screen region.

The [plugin source](https://github.com/samoylenkodmitry/Cranpose/tree/main/crates/cranpose/android/cranpose-gradle-plugin)
defines all settings. The
[native demo](https://github.com/samoylenkodmitry/Cranpose/tree/main/apps/native-demo)
shows Cranpose content inside an Android Compose app.
