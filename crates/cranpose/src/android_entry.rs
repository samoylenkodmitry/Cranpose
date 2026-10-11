/// Declares this crate's Android entry point.
///
/// Expands to nothing off Android, so it is written once at the crate root and
/// left there for every target.
///
/// ```ignore
/// cranpose::android_main! {
///     launcher: cranpose::AppLauncher::new().with_title("My App"),
///     content: my_app::screens::Root,
/// }
/// ```
///
/// `launcher` is any expression producing an [`AppLauncher`](crate::AppLauncher)
/// and `content` is the root composable. Both are evaluated only on Android.
///
/// The system can also start the process for a service alone, such as the one
/// that hands over what a paired watch sends. The macro exports the start of
/// such a process too: it evaluates `launcher` and runs its
/// [`with_process_start`](crate::AppLauncher::with_process_start).
#[macro_export]
macro_rules! android_main {
    (launcher: $launcher:expr, content: $content:expr $(,)?) => {
        /// The symbol `NativeActivity` resolves after loading this library.
        ///
        /// `#[unsafe(no_mangle)]` is what makes the name findable from Java, and is the
        /// only reason this item needs an `unsafe_code` allowance; the body is
        /// ordinary safe Rust.
        // SAFETY: exporting an unmangled symbol is sound as long as no other
        // symbol in the final library claims the same name. `android_main` is
        // the name `NativeActivity` looks up, so exactly one crate in an
        // application defines it — the one whose `cdylib` the APK packages —
        // and it is defined here rather than by hand in each of them.
        #[cfg(target_os = "android")]
        #[doc(hidden)]
        #[expect(unsafe_code)]
        #[unsafe(no_mangle)]
        pub fn android_main(app: $crate::AndroidApp) {
            $crate::AppLauncher::run($launcher, app, $content);
        }

        /// The method `CranposeProcess.nativeStart` resolves when a service
        /// starts the process without the activity.
        // SAFETY: as for `android_main`: the JNI name of one native method,
        // which only the crate the APK packages defines.
        #[cfg(target_os = "android")]
        #[doc(hidden)]
        #[expect(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "system" fn Java_dev_cranpose_android_CranposeProcess_nativeStart<'local>(
            env: $crate::__android_jni::EnvUnowned<'local>,
            class: $crate::__android_jni::objects::JClass<'local>,
            context: $crate::__android_jni::objects::JObject<'local>,
        ) {
            ($launcher).start_service_process(env, class, context);
        }
    };
}
