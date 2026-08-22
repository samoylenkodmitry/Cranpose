# Cranpose for Android

This directory is the Cranpose Android distribution: the framework's Java, the
manifest contributions every Cranpose application needs, the optional service
modules, and the Gradle plugin that configures a consuming application's build.

## Artifacts

| Artifact | What it contributes |
| --- | --- |
| `dev.cranpose:cranpose-android` | `CranposeActivity` and the rest of the framework's Java, the activity declaration with its launcher entry and `android.app.lib_name` metadata, the provider that serves shared files, and the consumer ProGuard rules that keep the JNI surface. |
| `dev.cranpose:cranpose-android-background` | The foreground service Cranpose runs while a background-work lease is held, and the permissions to start it. |
| `dev.cranpose:cranpose-android-camera` | The camera permission and the optional camera hardware feature. |
| `dev.cranpose:cranpose-android-media` | The media-playback foreground service and its permissions. |
| `dev.cranpose:cranpose-android-billing` | `CranposeBilling`, the Google Play Billing library, and the permission. |
| `dev.cranpose:cranpose-android-haptics` | The vibrator the haptics service drives. |
| `dev.cranpose:cranpose-android-notifications` | Notification posting. |
| `dev.cranpose:cranpose-android-overlay` | Windows drawn above other applications. |
| `dev.cranpose:cranpose-android-update` | The permission `PackageInstaller` requires to install an application update. |
| `dev.cranpose:cranpose-gradle-plugin` (`dev.cranpose.android`) | The native build, ABIs, Cargo profiles, JNI packaging, the artifact dependencies above, and the manifest metadata the activity declaration reads. |

Each optional module is manifest-only. An application that does not use a
service never asks the user for its permission.

## Using it

```kotlin
// settings.gradle.kts
pluginManagement {
    repositories { google(); mavenCentral(); gradlePluginPortal() }
}
```

```kotlin
// app/build.gradle.kts
plugins {
    id("com.android.application")
    id("dev.cranpose.android")
}

cranpose {
    cargoPackage.set("my-app-platform")
    services.addAll("notifications")
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
it. There is no activity to declare, no `android.app.lib_name` to keep in sync,
no `cargo ndk` invocation to copy, and no source set pointing into the
framework's tree.

The Rust entry point is declarative too:

```rust
cranpose::android_main! {
    launcher: cranpose::AppLauncher::new().with_title("My App"),
    content: my_app::screens::Root,
}
```

## Defaults the plugin applies

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

## Building this distribution

```sh
./gradlew publishToMavenLocal
```

Applications inside this repository consume it as a composite build instead;
see `apps/android-demo/android/settings.gradle.kts`.
