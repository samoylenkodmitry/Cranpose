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
#[macro_export]
macro_rules! android_main {
    (launcher: $launcher:expr, content: $content:expr $(,)?) => {
        /// The symbol `NativeActivity` resolves after loading this library.
        ///
        /// `#[unsafe(no_mangle)]` is what makes the name findable from Java, and is the
        /// only reason this item needs an `unsafe_code` allowance; the body is
        /// ordinary safe Rust.
        #[cfg(target_os = "android")]
        #[doc(hidden)]
        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub fn android_main(app: $crate::AndroidApp) {
            $crate::AppLauncher::run($launcher, app, $content);
        }
    };
}
