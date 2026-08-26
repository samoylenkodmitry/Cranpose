//! The Android native entry point, generated from a declarative spec.
//!
//! `NativeActivity` loads the application's `cdylib` and calls the exported
//! `android_main` symbol. Writing that symbol by hand costs every application
//! the same four lines: an `unsafe_code` allowance for the export attribute, a
//! `#[unsafe(no_mangle)]` it must not misspell, a dependency on `android_activity` for
//! nothing but the parameter type, and a `target_os` guard. None of it is about
//! the application, and every copy is a place the contract can drift from what
//! the framework and the manifest expect.
//!
//! [`android_main!`](crate::android_main) states the two things that are
//! genuinely the application's — how it launches, and what it draws — and the
//! framework writes the rest.

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
        // SAFETY: exporting an unmangled symbol is sound as long as no other
        // symbol in the final library claims the same name. `android_main` is
        // the name `NativeActivity` looks up, so exactly one crate in an
        // application defines it — the one whose `cdylib` the APK packages —
        // and it is defined here rather than by hand in each of them.
        #[cfg(target_os = "android")]
        #[doc(hidden)]
        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub fn android_main(app: $crate::AndroidApp) {
            $crate::AppLauncher::run($launcher, app, $content);
        }
    };
}
