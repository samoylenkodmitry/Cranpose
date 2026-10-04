# Cranpose Android platform

This internal platform crate connects Cranpose's application shell to
`android-activity`. Android lifecycle, surface, keyboard and touch events flow
through this crate to the shared runtime. Applications use the `android`
feature on `cranpose`; the platform crate serves framework integrations.

The Android entry point passes its `AndroidApp` handle to the launcher. The
root composable uses a zero-argument closure:

```rust,ignore
use cranpose::AppLauncher;

#[unsafe(no_mangle)]
fn android_main(app: android_activity::AndroidApp) {
    AppLauncher::new().run(app, || {
        MyApp();
    });
}
```

For the Android Gradle plugin, manifest declarations and optional services, see
the [`cranpose` crate guide](../../cranpose/README.md).
