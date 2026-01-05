//! Platform-agnostic application launcher with inversion of control.
//!
//! This module provides the `AppLauncher` API that allows apps to configure
//! and launch on multiple platforms without knowing platform-specific details.

/// Configuration for application settings.
pub struct AppSettings {
    /// Window title (desktop) / app name (mobile)
    pub window_title: String,
    /// Initial window width in logical pixels (desktop only)
    pub initial_width: u32,
    /// Initial window height in logical pixels (desktop only)
    pub initial_height: u32,
    /// Optional embedded fonts to use for text rendering
    pub fonts: Option<&'static [&'static [u8]]>,
    /// Whether to load system fonts on Android (default: false)
    pub android_use_system_fonts: bool,
    /// Optional test driver to control the application (robot testing)
    #[cfg(all(feature = "desktop", feature = "renderer-wgpu", feature = "robot"))]
    pub test_driver: Option<Box<dyn FnOnce(crate::desktop::Robot) + Send + 'static>>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            window_title: "Compose App".into(),
            initial_width: 800,
            initial_height: 600,
            fonts: None,
            android_use_system_fonts: false,
            #[cfg(all(feature = "desktop", feature = "renderer-wgpu", feature = "robot"))]
            test_driver: None,
        }
    }
}

/// Platform-agnostic application launcher.
///
/// This builder provides a unified API for launching Compose applications
/// on different platforms (desktop, Android, etc.) with proper inversion of control.
///
/// # Example
///
/// ```no_run
/// use compose_app::AppLauncher;
///
/// // Desktop
/// #[cfg(not(target_os = "android"))]
/// fn main() {
///     AppLauncher::new()
///         .with_title("My App")
///         .with_size(1024, 768)
///         .run(|| {
///             // Your composable UI here
///         });
/// }
///
/// // Android
/// #[cfg(target_os = "android")]
/// #[no_mangle]
/// fn android_main(app: android_activity::AndroidApp) {
///     AppLauncher::new()
///         .with_title("My App")
///         .run(app, || {
///             // Your composable UI here
///         });
/// }
/// ```
pub struct AppLauncher {
    settings: AppSettings,
}

impl AppLauncher {
    /// Create a new application launcher with default settings.
    pub fn new() -> Self {
        Self {
            settings: AppSettings::default(),
        }
    }

    /// Set the window title.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.settings.window_title = title.into();
        self
    }

    /// Set the initial window size (desktop only).
    pub fn with_size(mut self, width: u32, height: u32) -> Self {
        self.settings.initial_width = width;
        self.settings.initial_height = height;
        self
    }

    /// Set fonts to use for text rendering.
    ///
    /// If not set, the renderer will use an empty FontSystem (text will fail to render).
    /// Applications should provide fonts explicitly for consistent cross-platform rendering.
    pub fn with_fonts(mut self, fonts: &'static [&'static [u8]]) -> Self {
        self.settings.fonts = Some(fonts);
        self
    }

    /// Enable system font loading on Android (default: false).
    ///
    /// When false (recommended), only fonts provided via `with_fonts()` are used.
    /// When true, Android system fonts are loaded in addition to provided fonts.
    ///
    /// Note: Modern Android uses variable fonts which can cause rendering issues.
    /// Use static fonts via `with_fonts()` for reliable rendering.
    pub fn with_android_use_system_fonts(mut self, use_system_fonts: bool) -> Self {
        self.settings.android_use_system_fonts = use_system_fonts;
        self
    }

    /// Set a test driver to control the application.
    ///
    /// The driver closure will be executed in a separate thread and receive a `Robot` instance
    /// for controlling the application programmatically.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use compose_app::AppLauncher;
    ///
    /// AppLauncher::new()
    ///     .with_title("Robot Test")
    ///     .with_size(800, 600)
    ///     .with_test_driver(|robot| {
    ///         robot.wait_for_idle().unwrap();
    ///         robot.click(100.0, 100.0).unwrap();
    ///         robot.exit().unwrap();
    ///     })
    ///     .run(|| {
    ///         // Your composable UI here
    ///     });
    /// ```
    #[cfg(all(feature = "desktop", feature = "renderer-wgpu", feature = "robot"))]
    pub fn with_test_driver(
        mut self,
        driver: impl FnOnce(crate::desktop::Robot) + Send + 'static,
    ) -> Self {
        self.settings.test_driver = Some(Box::new(driver));
        self
    }

    /// Run the application (desktop platform).
    #[cfg(all(
        feature = "desktop",
        feature = "renderer-wgpu",
        not(target_os = "android")
    ))]
    pub fn run(self, content: impl FnMut() + 'static) -> ! {
        crate::desktop::run(self.settings, content)
    }

    /// Run the application (Android platform).
    #[cfg(all(feature = "android", feature = "renderer-wgpu", target_os = "android"))]
    pub fn run(self, app: android_activity::AndroidApp, content: impl FnMut() + 'static) {
        crate::android::run(app, self.settings, content)
    }

    /// Run the application (Web platform).
    ///
    /// Launches the app asynchronously targeting the canvas with the given ID.
    /// Returns a Promise that resolves when the app is initialized.
    #[cfg(all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32"))]
    pub async fn run_web(
        self,
        canvas_id: &str,
        content: impl FnMut() + 'static,
    ) -> Result<(), wasm_bindgen::JsValue> {
        crate::web::run(canvas_id, self.settings, content).await
    }
}

impl Default for AppLauncher {
    fn default() -> Self {
        Self::new()
    }
}
