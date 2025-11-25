#![deny(missing_docs)]

//! High level utilities for running Compose applications with minimal boilerplate.

#[cfg(not(any(feature = "desktop", feature = "android")))]
compile_error!("compose-app must be built with either the `desktop` or `android` feature enabled.");

#[cfg(not(any(feature = "renderer-pixels", feature = "renderer-wgpu")))]
compile_error!("compose-app requires either the `renderer-pixels` or `renderer-wgpu` feature.");

// New unified launcher API
mod launcher;
pub use launcher::{AppLauncher, AppSettings};

/// Platform-agnostic application entry point.
///
/// This macro generates the correct entry point for each platform automatically.
/// Developers write their app once, and it compiles for desktop and Android.
///
/// # Example
///
/// ```ignore
/// use compose_app::ComposeApp;
/// use compose_ui::*;
///
/// #[composable]
/// fn MyApp() {
///     Text("Hello, World!");
/// }
///
/// // This ONE line works on ALL platforms!
/// ComposeApp!(MyApp);
/// ```
///
/// **Desktop:** Generates `fn main()` that uses winit event loop
/// **Android:** Generates `fn android_main()` that uses android-activity lifecycle
///
/// The developer never needs to know or care what platform they're targeting.
#[macro_export]
macro_rules! ComposeApp {
    // Simple form: just pass the composable function
    ($content:expr) => {
        $crate::ComposeApp!(
            title: "Compose App",
            width: 800,
            height: 600,
            content: $content
        );
    };

    // Full form: configure title and size
    (
        title: $title:expr,
        width: $width:expr,
        height: $height:expr,
        content: $content:expr
    ) => {
        // Desktop entry point (Windows, macOS, Linux)
        #[cfg(not(target_os = "android"))]
        fn main() {
            // Initialize env_logger only on desktop (Android uses android_logger in framework)
            let _ = env_logger::try_init();

            $crate::AppLauncher::new()
                .with_title($title)
                .with_size($width, $height)
                .run(|| $content);
        }

        // Android entry point
        #[cfg(target_os = "android")]
        #[no_mangle]
        fn android_main(app: android_activity::AndroidApp) {
            $crate::AppLauncher::new()
                .with_title($title)
                .run(app, || $content);
        }
    };
}

// Platform-specific runtime modules
#[cfg(all(feature = "android", feature = "renderer-wgpu"))]
pub mod android;

#[cfg(all(feature = "desktop", feature = "renderer-wgpu"))]
pub mod desktop;
