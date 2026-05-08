//! Platform-agnostic application launcher with inversion of control.
//!
//! This module provides the `AppLauncher` API that allows apps to configure
//! and launch on multiple platforms without knowing platform-specific details.

#[cfg(all(feature = "desktop", feature = "renderer-wgpu"))]
use cranpose_app_shell::FramePacingMode;
#[cfg(all(feature = "desktop", feature = "renderer-wgpu"))]
use std::path::PathBuf;
#[cfg(all(feature = "desktop", feature = "renderer-wgpu"))]
use thiserror::Error;

/// Configuration for application settings.
pub struct AppSettings {
    /// Window title (desktop) / app name (mobile)
    pub window_title: String,
    /// Initial window width in logical pixels.
    pub initial_width: u32,
    /// Initial window height in logical pixels.
    pub initial_height: u32,
    /// Whether the initial size was explicitly supplied by the app.
    pub initial_size_explicit: bool,
    /// Fonts loaded for text rendering (ordered: primary first, fallbacks last).
    pub fonts: Option<&'static [&'static [u8]]>,
    /// Whether to load system fonts on Android (default: false)
    pub android_use_system_fonts: bool,
    /// Optional Android overlay surface configuration.
    pub android_overlay_window: Option<AndroidOverlayWindowOptions>,
    /// Run in headless mode (window hidden, for robot testing)
    ///
    /// When enabled, the window is created but not shown. This allows
    /// robot tests to run in parallel without cluttering the screen
    /// and enables CI environments without a display server.
    pub headless: bool,
    /// Whether the launcher-created primary desktop window should be visible.
    ///
    /// Multi-window apps can hide this bootstrap surface and declare their
    /// visible operating-system windows through `run_windows`.
    pub primary_window_visible: bool,
    /// Development options for debugging and performance monitoring
    #[cfg(all(feature = "desktop", feature = "renderer-wgpu"))]
    pub dev_options: cranpose_app_shell::DevOptions,
    /// Initial desktop frame pacing mode.
    #[cfg(all(feature = "desktop", feature = "renderer-wgpu"))]
    pub frame_pacing_mode: FramePacingMode,
    /// Optional test driver to control the application (robot testing)
    #[cfg(all(feature = "desktop", feature = "renderer-wgpu", feature = "robot"))]
    pub test_driver: Option<Box<dyn FnOnce(crate::desktop::Robot) + Send + 'static>>,
    /// Optional app-thread hook invoked by robot tests for deterministic state control.
    #[cfg(all(feature = "desktop", feature = "renderer-wgpu", feature = "robot"))]
    pub robot_app_hook: Option<Box<crate::RobotAppHook>>,
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
            initial_size_explicit: false,
            fonts: None,
            android_use_system_fonts: false,
            android_overlay_window: None,
            headless: false,
            primary_window_visible: true,
            #[cfg(all(feature = "desktop", feature = "renderer-wgpu"))]
            dev_options: cranpose_app_shell::DevOptions::default(),
            #[cfg(all(feature = "desktop", feature = "renderer-wgpu"))]
            frame_pacing_mode: FramePacingMode::NoVsync,
            #[cfg(all(feature = "desktop", feature = "renderer-wgpu", feature = "robot"))]
            test_driver: None,
            #[cfg(all(feature = "desktop", feature = "renderer-wgpu", feature = "robot"))]
            robot_app_hook: None,
            #[cfg(all(feature = "desktop", feature = "renderer-wgpu"))]
            record_to: None,
        }
    }
}

/// Android floating overlay window configuration.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AndroidOverlayWindowOptions {
    /// Requested overlay width in logical pixels.
    pub width: u32,
    /// Requested overlay height in logical pixels.
    pub height: u32,
    /// Requested screen X position in logical pixels.
    pub x: i32,
    /// Requested screen Y position in logical pixels.
    pub y: i32,
    /// Whether the overlay can receive keyboard focus.
    pub focusable: bool,
}

impl AndroidOverlayWindowOptions {
    /// Creates an overlay window request with top-left origin and touch-only focus behavior.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            x: 0,
            y: 0,
            focusable: false,
        }
    }

    /// Sets the initial overlay position in logical pixels.
    pub fn with_position(mut self, x: i32, y: i32) -> Self {
        self.x = x;
        self.y = y;
        self
    }

    /// Sets whether the overlay should receive keyboard focus.
    pub fn with_focusable(mut self, focusable: bool) -> Self {
        self.focusable = focusable;
        self
    }

    /// Returns whether this request can create a non-empty overlay surface.
    pub fn is_valid(self) -> bool {
        self.width > 0 && self.height > 0
    }
}

/// Errors that can occur while launching a desktop application.
#[cfg(all(feature = "desktop", feature = "renderer-wgpu"))]
#[derive(Debug, Error)]
pub enum LaunchError {
    /// Creating the desktop event loop failed.
    #[error("failed to create desktop event loop: {0}")]
    EventLoopCreate(#[source] winit::error::EventLoopError),
    /// Creating the desktop window failed.
    #[error("failed to create desktop window: {0}")]
    WindowCreate(#[source] winit::error::RequestError),
    /// Creating the rendering surface failed.
    #[error("failed to create desktop rendering surface: {0}")]
    SurfaceCreate(#[source] wgpu::CreateSurfaceError),
    /// No compatible GPU adapter was available for the surface.
    #[error("no compatible GPU adapter was available: {0}")]
    NoAdapter(#[source] wgpu::RequestAdapterError),
    /// Creating the GPU device failed.
    #[error("failed to create GPU device: {0}")]
    DeviceCreate(#[source] wgpu::RequestDeviceError),
    /// The desktop event loop terminated with an error.
    #[error("desktop event loop terminated with error: {0}")]
    EventLoopRun(#[source] winit::error::EventLoopError),
    /// The robot driver panicked while controlling the application.
    #[cfg(feature = "robot")]
    #[error("desktop robot test driver panicked: {0}")]
    TestDriverPanic(String),
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

    /// Set the initial window size.
    ///
    /// Desktop uses this as the initial primary window size. Android sends it
    /// as a best-effort host-window request after the native surface exists;
    /// fullscreen and split-screen activities may keep the system-managed
    /// bounds, while freeform and desktop-windowing activities can honor it.
    /// iOS and maximized Web canvases still keep platform-controlled bounds.
    ///
    /// # Arguments
    ///
    /// * `width` - The initial width in logical pixels.
    /// * `height` - The initial height in logical pixels.
    pub fn with_size(mut self, width: u32, height: u32) -> Self {
        self.settings.initial_width = width;
        self.settings.initial_height = height;
        self.settings.initial_size_explicit = true;
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
    /// use cranpose::AppLauncher;
    ///
    /// // In specialized environments, you might include bytes:
    /// // static REGULAR: &[u8] = include_bytes!("../assets/MyFont.ttf");
    /// static DUMMY_FONT: &[u8] = &[];
    /// static FONTS: &[&[u8]] = &[DUMMY_FONT];
    ///
    /// AppLauncher::new()
    ///     .with_fonts(FONTS);
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

    /// Render the Android root into a floating `TYPE_APPLICATION_OVERLAY` surface.
    ///
    /// This Android-only mode requires the host app to declare
    /// `android.permission.SYSTEM_ALERT_WINDOW`, include Cranpose's Android Java
    /// helper sources, and obtain overlay permission before launch. Other
    /// platforms ignore this setting and keep their normal primary surface.
    pub fn with_android_overlay_window(mut self, options: AndroidOverlayWindowOptions) -> Self {
        self.settings.android_overlay_window = Some(options);
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
    /// let launcher = AppLauncher::new()
    ///     .with_title("Robot Test")
    ///     .with_size(800, 600)
    ///     .with_headless(true);
    ///
    /// #[cfg(feature = "robot")]
    /// let launcher = launcher.with_test_driver(|robot| {
    ///     robot.wait_for_idle().unwrap();
    ///     robot.click(100.0, 100.0).unwrap();
    ///     robot.exit().unwrap();
    /// });
    ///
    /// launcher.run(|| {
    ///     // Your composable UI here
    /// });
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

    /// Set the initial desktop frame pacing mode.
    ///
    /// This controls whether the desktop surface uses vsync or no-vsync presentation and,
    /// for hard caps, limits redraw scheduling to the requested frame rate.
    #[cfg(all(feature = "desktop", feature = "renderer-wgpu"))]
    pub fn with_frame_pacing_mode(mut self, mode: FramePacingMode) -> Self {
        self.settings.frame_pacing_mode = mode;
        self.settings.dev_options.frame_pacing_mode = mode;
        self
    }

    /// Set the initial desktop frame pacing mode.
    #[cfg(not(all(feature = "desktop", feature = "renderer-wgpu")))]
    pub fn with_frame_pacing_mode(self, mode: cranpose_app_shell::FramePacingMode) -> Self {
        let _ = mode;
        self
    }

    /// Enable clickable frame pacing controls in the desktop development overlay.
    ///
    /// Enabling the controls also enables the FPS overlay because the controls are rendered
    /// as part of that overlay.
    #[cfg(all(feature = "desktop", feature = "renderer-wgpu"))]
    pub fn with_frame_pacing_controls(mut self, enabled: bool) -> Self {
        self.settings.dev_options.frame_pacing_controls = enabled;
        if enabled {
            self.settings.dev_options.fps_counter = true;
        }
        self
    }

    /// Enable clickable frame pacing controls in the desktop development overlay.
    #[cfg(not(all(feature = "desktop", feature = "renderer-wgpu")))]
    pub fn with_frame_pacing_controls(self, enabled: bool) -> Self {
        let _ = enabled;
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

    #[cfg(all(feature = "desktop", feature = "renderer-wgpu", feature = "robot"))]
    #[doc(hidden)]
    pub fn with_robot_app_hook(
        mut self,
        hook: impl FnMut(String, String) -> Result<Option<String>, String> + 'static,
    ) -> Self {
        self.settings.robot_app_hook = Some(Box::new(hook));
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
    pub fn try_run(self, content: impl FnMut() + 'static) -> Result<(), LaunchError> {
        let mut content = content;
        crate::desktop::try_run(self.settings, move || {
            crate::ProvideUriHandler(|| {
                content();
            });
        })
    }

    /// Run the application (Desktop platform).
    ///
    /// Use [`AppLauncher::try_run`] when the caller needs a typed launch failure.
    #[cfg(all(
        feature = "desktop",
        feature = "renderer-wgpu",
        not(target_os = "android")
    ))]
    pub fn run(self, content: impl FnMut() + 'static) -> ! {
        self.try_run(content)
            .unwrap_or_else(|error| panic!("desktop launch failed: {error}"));
        std::process::exit(0)
    }

    /// Run a desktop app that declares its visible operating-system windows directly.
    ///
    /// The primary launcher surface is kept hidden; content should declare peer
    /// windows with `WindowNode` or `Window`.
    #[cfg(all(
        feature = "desktop",
        feature = "renderer-wgpu",
        not(target_os = "android")
    ))]
    pub fn try_run_windows(mut self, content: impl FnMut() + 'static) -> Result<(), LaunchError> {
        self.settings.primary_window_visible = false;
        self.try_run(content)
    }

    /// Run a desktop app that declares its visible operating-system windows directly.
    #[cfg(all(
        feature = "desktop",
        feature = "renderer-wgpu",
        not(target_os = "android")
    ))]
    pub fn run_windows(self, content: impl FnMut() + 'static) -> ! {
        self.try_run_windows(content)
            .unwrap_or_else(|error| panic!("desktop launch failed: {error}"));
        std::process::exit(0)
    }

    /// Run the application (Android platform).
    ///
    /// # Arguments
    ///
    /// * `app` - The `AndroidApp` handle provided by `android_activity`.
    /// * `content` - The root composable function of your application.
    #[cfg(all(feature = "android", target_os = "android"))]
    pub fn run(self, app: android_activity::AndroidApp, content: impl FnMut() + 'static) {
        let mut content = content;
        crate::android::run(app, self.settings, move || {
            crate::ProvideUriHandler(|| {
                content();
            });
        });
    }

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
        let mut content = content;
        crate::web::run(canvas_id, self.settings, move || {
            crate::ProvideUriHandler(|| {
                content();
            });
        })
        .await
    }
}

impl Default for AppLauncher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn android_overlay_options_default_to_touch_only_top_left_window() {
        let options = AndroidOverlayWindowOptions::new(320, 180);

        assert_eq!(options.width, 320);
        assert_eq!(options.height, 180);
        assert_eq!(options.x, 0);
        assert_eq!(options.y, 0);
        assert!(!options.focusable);
        assert!(options.is_valid());
    }

    #[test]
    fn android_overlay_options_apply_position_and_focus() {
        let options = AndroidOverlayWindowOptions::new(320, 180)
            .with_position(12, 34)
            .with_focusable(true);

        assert_eq!(options.x, 12);
        assert_eq!(options.y, 34);
        assert!(options.focusable);
    }

    #[test]
    fn android_overlay_options_reject_zero_size() {
        assert!(!AndroidOverlayWindowOptions::new(0, 180).is_valid());
        assert!(!AndroidOverlayWindowOptions::new(320, 0).is_valid());
    }
}
