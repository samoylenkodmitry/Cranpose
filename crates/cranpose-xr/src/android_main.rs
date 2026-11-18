//! The Android entry of an immersive app.

/// Declares this crate's Android entry point as an immersive app for headsets.
///
/// It takes the same arguments as [`cranpose::android_main!`] and expands to nothing off
/// Android. The app's Android build turns on this crate's `openxr` feature, and its Gradle
/// build lists the `xr` service, which makes the APK an immersive app and packs the OpenXR
/// loader.
///
/// ```ignore
/// cranpose_xr::android_main! {
///     launcher: cranpose::AppLauncher::new(),
///     content: my_app::Root,
/// }
/// ```
#[macro_export]
macro_rules! android_main {
    (launcher: $launcher:expr, content: $content:expr $(,)?) => {
        // The symbol `NativeActivity` resolves after loading this library.
        // SAFETY: exporting an unmangled symbol is sound while no other symbol in the library
        // has the name. `android_main` is the name `NativeActivity` looks up, and only the crate
        // whose cdylib the APK packs defines it.
        #[cfg(target_os = "android")]
        #[doc(hidden)]
        #[expect(unsafe_code, reason = "the symbol NativeActivity looks up")]
        #[unsafe(no_mangle)]
        pub fn android_main(app: $crate::__cranpose::AndroidApp) {
            $crate::run($launcher, app, $content);
        }

        // The method `CranposeProcess.nativeStart` resolves when a service starts the process
        // without the activity.
        // SAFETY: as for `android_main`: the JNI name of one native method, which only the crate
        // the APK packs defines.
        #[cfg(target_os = "android")]
        #[doc(hidden)]
        #[expect(unsafe_code, reason = "a JNI method of the framework's Java")]
        #[unsafe(no_mangle)]
        pub extern "system" fn Java_dev_cranpose_android_CranposeProcess_nativeStart<'local>(
            env: $crate::__cranpose::__android_jni::EnvUnowned<'local>,
            class: $crate::__cranpose::__android_jni::objects::JClass<'local>,
            context: $crate::__cranpose::__android_jni::objects::JObject<'local>,
            share: $crate::__cranpose::__android_jni::sys::jboolean,
        ) {
            ($launcher).start_service_process(env, class, context, share);
        }
    };
}
