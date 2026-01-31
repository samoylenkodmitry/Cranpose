//! Platform-agnostic application launcher with inversion of control.
//!
//! This module provides the `AppLauncher` API that allows apps to configure
//! and launch on multiple platforms without knowing platform-specific details.

#[cfg(all(feature = "desktop", feature = "renderer-wgpu"))]
use std::path::PathBuf;

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
    /// Run in headless mode (window hidden, for robot testing)
    ///
    /// When enabled, the window is created but not shown. This allows
    /// robot tests to run in parallel without cluttering the screen
    /// and enables CI environments without a display server.
    pub headless: bool,
    /// Development options for debugging and performance monitoring
    #[cfg(all(feature = "desktop", feature = "renderer-wgpu"))]
    pub dev_options: cranpose_app_shell::DevOptions,
    /// Optional test driver to control the application (robot testing)
    #[cfg(all(feature = "desktop", feature = "renderer-wgpu", feature = "robot"))]
    pub test_driver: Option<Box<dyn FnOnce(crate::desktop::Robot) + Send + 'static>>,
    /// Optional path to record input events to (for generating robot tests)
    #[cfg(all(feature = "desktop", feature = "renderer-wgpu"))]
    pub record_to: Option<PathBuf>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            window_title: "Compose App".into(),
            initial_width: 800,
            initial_height: 600,
            fonts: None,
            android_use_system_fonts: false,
            headless: false,
            #[cfg(all(feature = "desktop", feature = "renderer-wgpu"))]
            dev_options: cranpose_app_shell::DevOptions::default(),
            #[cfg(all(feature = "desktop", feature = "renderer-wgpu", feature = "robot"))]
            test_driver: None,
            #[cfg(all(feature = "desktop", feature = "renderer-wgpu"))]
            record_to: None,
        }
    }
}

/// Platform-agnostic application launcher.
///
/// Platform-agnostic application launcher.
///
/// This builder provides a unified API for launching Compose applications
/// on different platforms (desktop, Android, Web) with proper inversion of control.
/// It abstracts away the differences between window creation, event loops,
/// and surface initialization.
///
/// # When to use
///
/// Use `AppLauncher` as the standard entry point for any Cranpose application.
/// It handles the boilerplate of:
/// -   Creating a window or attaching to a view.
/// -   Initializing the graphics context (WGPU instance, Surface, Adapter, Device).
/// -   Setting up the main event loop.
/// -   Bridging platform events to the Cranpose runtime.
///
/// # Example
///
/// ```no_run
/// use cranpose::AppLauncher;
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
    ///
    /// # Arguments
    ///
    /// * `title` - The string to display in the window title bar (Desktop/Web) or the activity label (Android).
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.settings.window_title = title.into();
        self
    }

    /// Set the initial window size (Desktop only).
    ///
    /// This hint is ignored on platforms where the window size is controlled by the OS
    /// (e.g., Android, iOS, or maximized Web canvas).
    ///
    /// # Arguments
    ///
    /// * `width` - The initial width in logical pixels.
    /// * `height` - The initial height in logical pixels.
    pub fn with_size(mut self, width: u32, height: u32) -> Self {
        self.settings.initial_width = width;
        self.settings.initial_height = height;
        self
    }

    /// Set fonts to use for text rendering.
    ///
    /// # Arguments
    ///
    /// * `fonts` - A slice of static byte slices, each representing a font file (e.g., `.ttf` or `.otf`).
    ///
    /// # Example
    ///
    /// ```no_run
    /// AppLauncher::new()
    ///     .with_fonts(&[
    ///         include_bytes!("../assets/Roboto-Regular.ttf"),
    ///         include_bytes!("../assets/Roboto-Bold.ttf"),
    ///     ])
    /// ```
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

    /// Enable headless mode for robot testing.
    ///
    /// When headless mode is enabled, the window is created but not shown.
    /// This allows robot tests to:
    /// - Run in parallel without windows overlapping or stealing focus
    /// - Run in CI environments without a display server (using Xvfb or similar)
    /// - Execute faster by skipping window decoration rendering
    ///
    /// Note: The app still creates a full WGPU surface for accurate rendering tests.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use cranpose::AppLauncher;
    ///
    /// AppLauncher::new()
    ///     .with_title("Robot Test")
    ///     .with_size(800, 600)
    ///     .with_headless(true)
    ///     .with_test_driver(|robot| {
    ///         robot.wait_for_idle().unwrap();
    ///         robot.click(100.0, 100.0).unwrap();
    ///         robot.exit().unwrap();
    ///     })
    ///     .run(|| {
    ///         // Your composable UI here
    ///     });
    /// ```
    pub fn with_headless(mut self, headless: bool) -> Self {
        self.settings.headless = headless;
        self
    }

    /// Enable FPS counter overlay (desktop only).
    ///
    /// When enabled, displays a real-time FPS counter in the top-right corner.
    /// This is rendered directly by the renderer (not via composition) so it
    /// doesn't affect performance measurements.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use cranpose::AppLauncher;
    ///
    /// AppLauncher::new()
    ///     .with_title("My App")
    ///     .with_fps_counter(true)
    ///     .run(|| {
    ///         // Your composable UI here
    ///     });
    /// ```
    #[cfg(all(feature = "desktop", feature = "renderer-wgpu"))]
    pub fn with_fps_counter(mut self, enabled: bool) -> Self {
        self.settings.dev_options.fps_counter = enabled;
        self
    }

    /// Enable FPS counter overlay (desktop only).
    ///
    /// When enabled, displays a real-time FPS counter in the top-right corner.
    /// This is rendered directly by the renderer (not via composition) so it
    /// doesn't affect performance measurements.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use cranpose::AppLauncher;
    ///
    /// AppLauncher::new()
    ///     .with_title("My App")
    ///     .with_fps_counter(true)
    ///     .run(|| {
    ///         // Your composable UI here
    ///     });
    /// ```
    #[cfg(not(all(feature = "desktop", feature = "renderer-wgpu")))]
    pub fn with_fps_counter(self, enabled: bool) -> Self {
        let _ = enabled;
        self
    }

    /// Enable input recording mode.
    ///
    /// When enabled, all mouse and keyboard events are recorded with precise
    /// timestamps. On app exit, a robot test file is generated that can replay
    /// the exact interaction sequence.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use cranpose::AppLauncher;
    ///
    /// AppLauncher::new()
    ///     .with_title("My App")
    ///     .with_recording("/tmp/my_test.rs")
    ///     .run(|| {
    ///         // Interact with the app, then close
    ///         // Recording is saved automatically
    ///     });
    /// ```
    #[cfg(all(feature = "desktop", feature = "renderer-wgpu"))]
    pub fn with_recording(mut self, path: impl Into<PathBuf>) -> Self {
        self.settings.record_to = Some(path.into());
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
    /// use cranpose::AppLauncher;
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

    /// Run the application (Desktop platform).
    ///
    /// This method blocks the current thread and starts the platform event loop.
    /// It should be the last call in your `main` function.
    ///
    /// # Arguments
    ///
    /// * `content` - The root composable function of your application.
    #[cfg(all(
        feature = "desktop",
        feature = "renderer-wgpu",
        not(target_os = "android")
    ))]
    pub fn run(self, content: impl FnMut() + 'static) -> ! {
        crate::desktop::run(self.settings, content)
    }

    /// Run the application (Android platform).
    ///
    /// # Arguments
    ///
    /// * `app` - The `AndroidApp` handle provided by `android_activity`.
    /// * `content` - The root composable function of your application.

    /// Run the application (Web platform).
    ///
    /// Launches the app asynchronously targeting the canvas with the given ID.
    ///
    /// # Arguments
    ///
    /// * `canvas_id` - The DOM ID of the HTML `<canvas>` element to render into.
    /// * `content` - The root composable function.
    ///
    /// # Returns
    ///
    /// A `Promise` that resolves when the app is initialized (or fails).
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
