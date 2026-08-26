//! Platform-agnostic application launcher with inversion of control.
//!
//! This module provides the `AppLauncher` API that allows apps to configure
//! and launch on multiple platforms without knowing platform-specific details.

use std::path::Path;
#[cfg(all(feature = "desktop-shell", feature = "renderer-wgpu"))]
use std::path::PathBuf;

#[cfg(all(feature = "desktop-shell", feature = "renderer-wgpu"))]
use cranpose_app_shell::FramePacingMode;
use cranpose_render_common::{
    font_source::{
        ANDROID_SYSTEM_FONT_DIR, DEFAULT_SYSTEM_FAMILY_WEIGHTS, FontLoadError,
        SoftwareTextFontRegistry,
    },
    software_text_raster::SoftwareTextFontSet,
};
use cranpose_ui::text::{FontFamily, FontStyle, FontWeight};
#[cfg(all(
    feature = "renderer-wgpu",
    any(feature = "desktop-shell", all(feature = "ios", target_os = "ios"))
))]
use thiserror::Error;

/// Configuration for application settings.
pub struct AppSettings {
    /// Window title (desktop) / app name (mobile)
    pub window_title: String,
    /// The id every framework-owned storage path is scoped by.
    ///
    /// Android and iOS take it from what the platform packaged; a desktop or
    /// web build states it here. Left unset, the desktop backend derives one
    /// from the executable name, which is right for a development build and
    /// wrong for a shipped one.
    pub application_id: Option<String>,
    /// Initial window width in logical pixels.
    pub initial_width: u32,
    /// Initial window height in logical pixels.
    pub initial_height: u32,
    /// Whether the initial size was explicitly supplied by the app.
    pub initial_size_explicit: bool,
    /// Fonts loaded for text rendering (ordered: primary first, fallbacks last).
    pub fonts: Option<&'static [&'static [u8]]>,
    /// App-supplied font families, already read and parsed.
    ///
    /// This is where fonts that are not compiled into the binary live: files on
    /// disk, platform system fonts, APK assets. Faces registered here carry the
    /// `FontFamily` an app names them by, so a `TextStyle` asking for that
    /// family resolves to them for both measurement and drawing.
    pub font_registry: SoftwareTextFontRegistry,
    /// Whether to load system fonts on Android (default: false)
    pub android_use_system_fonts: bool,
    /// The tag this application's log lines carry.
    ///
    /// Android routes every log line through one tag, and `adb logcat -s
    /// <tag>` is how anyone reads them; an application that wants its own name
    /// there had to initialise the platform logger itself, before the framework
    /// did, and hope the ordering held. Naming it here removes both the
    /// platform call and the race. Unset, lines carry `Cranpose`.
    pub log_tag: Option<String>,
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
    #[cfg(all(feature = "desktop-shell", feature = "renderer-wgpu"))]
    pub dev_options: cranpose_app_shell::DevOptions,
    /// Initial desktop frame pacing mode.
    #[cfg(all(feature = "desktop-shell", feature = "renderer-wgpu"))]
    pub frame_pacing_mode: FramePacingMode,
    /// Whether the app chose a frame pacing mode explicitly. Installing a
    /// robot test driver lifts the vsync cap only when this is false.
    #[cfg(all(feature = "desktop-shell", feature = "renderer-wgpu"))]
    pub frame_pacing_explicit: bool,
    /// Optional test driver to control the application (robot testing)
    #[cfg(all(
        feature = "desktop-shell",
        feature = "robot",
        feature = "renderer-wgpu"
    ))]
    pub test_driver: Option<Box<dyn FnOnce(crate::Robot) + Send + 'static>>,
    /// Optional app-thread hook invoked by robot tests for deterministic state control.
    #[cfg(all(
        feature = "desktop-shell",
        feature = "robot",
        feature = "renderer-wgpu"
    ))]
    pub robot_app_hook: Option<Box<crate::RobotAppHook>>,
    /// Optional path to record input events to (for generating robot tests)
    #[cfg(all(feature = "desktop-shell", feature = "renderer-wgpu"))]
    pub record_to: Option<PathBuf>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            window_title: "Compose App".into(),
            application_id: None,
            initial_width: 800,
            initial_height: 600,
            initial_size_explicit: false,
            fonts: None,
            font_registry: SoftwareTextFontRegistry::new(),
            android_use_system_fonts: false,
            log_tag: None,
            android_overlay_window: None,
            headless: false,
            primary_window_visible: true,
            #[cfg(all(feature = "desktop-shell", feature = "renderer-wgpu"))]
            dev_options: cranpose_app_shell::DevOptions::default(),
            #[cfg(all(feature = "desktop-shell", feature = "renderer-wgpu"))]
            frame_pacing_mode: FramePacingMode::Vsync,
            #[cfg(all(feature = "desktop-shell", feature = "renderer-wgpu"))]
            frame_pacing_explicit: false,
            #[cfg(all(
                feature = "desktop-shell",
                feature = "robot",
                feature = "renderer-wgpu"
            ))]
            test_driver: None,
            #[cfg(all(
                feature = "desktop-shell",
                feature = "robot",
                feature = "renderer-wgpu"
            ))]
            robot_app_hook: None,
            #[cfg(all(feature = "desktop-shell", feature = "renderer-wgpu"))]
            record_to: None,
        }
    }
}

impl AppSettings {
    /// The font set every platform entry point hands its renderer.
    ///
    /// One definition so the platforms cannot drift, and so the measurer and
    /// the rasterizer are built from the same faces: app-registered families
    /// first, then the static `with_fonts()` slices as unnamed fallbacks, then
    /// the embedded default face if nothing else loaded.
    pub fn resolve_font_set(&self) -> SoftwareTextFontSet {
        let mut registry = self.font_registry.clone();
        if cfg!(target_os = "android") && self.android_use_system_fonts {
            for family in [
                FontFamily::SansSerif,
                FontFamily::Serif,
                FontFamily::Monospace,
            ] {
                if let Err(error) = registry.register_system_family(
                    ANDROID_SYSTEM_FONT_DIR,
                    &family,
                    DEFAULT_SYSTEM_FAMILY_WEIGHTS,
                ) {
                    log::warn!("android system font family {family:?} unavailable: {error}");
                }
            }
        }
        let fonts = registry.into_font_set_or_default(self.fonts.unwrap_or(&[]));
        // Which typeface text actually landed on is otherwise invisible until
        // someone compares screenshots, so say it once at startup.
        log::info!(
            "Text fonts: {} face(s) [{}]",
            fonts.faces().len(),
            fonts
                .faces()
                .iter()
                .map(|face| format!(
                    "{} {}{}",
                    face.family_names().first().map_or("?", String::as_str),
                    face.weight().value(),
                    if face.style() == FontStyle::Italic {
                        " italic"
                    } else {
                        ""
                    }
                ))
                .collect::<Vec<_>>()
                .join(", ")
        );
        fonts
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

/// Errors that can occur while launching a windowed (desktop or iOS) application.
#[cfg(all(
    feature = "renderer-wgpu",
    any(feature = "desktop-shell", all(feature = "ios", target_os = "ios"))
))]
#[derive(Debug, Error)]
pub enum LaunchError {
    /// Creating the desktop event loop failed.
    #[error("failed to create desktop event loop: {0}")]
    EventLoopCreate(#[source] winit::error::EventLoopError),
    /// Creating the desktop window failed.
    #[error("failed to create desktop window: {0}")]
    WindowCreate(#[source] winit::error::RequestError),
    /// Creating the rendering surface failed.
    #[cfg(feature = "renderer-wgpu")]
    #[error("failed to create desktop rendering surface: {0}")]
    SurfaceCreate(#[source] wgpu::CreateSurfaceError),
    /// The rendering surface did not report any supported formats.
    #[error("desktop rendering surface reports no supported formats")]
    NoSurfaceFormat,
    /// The rendering surface did not report any supported alpha modes.
    #[error("desktop rendering surface reports no supported alpha modes")]
    NoSurfaceAlphaMode,
    /// No compatible GPU adapter was available for the surface.
    #[cfg(feature = "renderer-wgpu")]
    #[error("no compatible GPU adapter was available: {0}")]
    NoAdapter(#[source] wgpu::RequestAdapterError),
    /// Creating the GPU device failed.
    #[cfg(feature = "renderer-wgpu")]
    #[error("failed to create GPU device: {0}")]
    DeviceCreate(#[source] wgpu::RequestDeviceError),
    /// The desktop renderer context was not initialized before a surface needed it.
    #[error("desktop GPU context is not initialized")]
    GpuContextUnavailable,
    /// The application content closure was already consumed before the primary shell was created.
    #[error("desktop application content is unavailable during launch")]
    ContentUnavailable,
    /// The desktop event loop terminated with an error.
    #[error("desktop event loop terminated with error: {0}")]
    EventLoopRun(#[source] winit::error::EventLoopError),
    /// The robot driver panicked while controlling the application.
    #[cfg(feature = "robot")]
    #[error("desktop robot test driver panicked: {0}")]
    TestDriverPanic(String),
}

#[cfg(all(
    feature = "renderer-wgpu",
    any(feature = "desktop-shell", feature = "ios")
))]
pub(crate) fn exit_after_launch_error(context: &str, error: LaunchError) -> ! {
    eprintln!("{context}: {error}");
    std::process::exit(1)
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
/// #[cfg(all(
///     feature = "desktop-shell",
///     feature = "renderer-wgpu",
///     not(target_os = "android")
/// ))]
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
/// #[cfg(all(feature = "android", target_os = "android"))]
/// #[unsafe(no_mangle)]
/// fn android_main(app: android_activity::AndroidApp) {
///     AppLauncher::new().with_title("My App").run(app, || {
///         // Your composable UI here
///     });
/// }
///
/// #[cfg(not(any(
///     all(
///         feature = "desktop-shell",
///         feature = "renderer-wgpu",
///         not(target_os = "android")
///     ),
///     all(feature = "android", target_os = "android")
/// )))]
/// fn main() {}
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

    /// Set the id every framework-owned storage path is scoped by.
    ///
    /// Use the same reverse-DNS identifier the app is packaged under, so a
    /// desktop build writes beside its own data rather than beside every other
    /// Cranpose application.
    ///
    /// # Arguments
    ///
    /// * `application_id` - for example `com.example.notes`.
    pub fn with_application_id(mut self, application_id: impl Into<String>) -> Self {
        self.settings.application_id = Some(application_id.into());
        self
    }

    /// Set the initial window size.
    ///
    /// Desktop uses this as the initial primary window size. Android applies
    /// it as a best-effort host-window request only when the activity starts
    /// in a multi-window mode (freeform / desktop windowing such as DeX);
    /// fullscreen activities ignore it and keep the display-sized,
    /// edge-to-edge surface, because shrinking the fullscreen window would
    /// leave uncovered (black) strips of display around the surface.
    /// Maximized Web canvases still keep platform-controlled bounds.
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
    /// AppLauncher::new().with_fonts(FONTS);
    /// ```
    pub fn with_fonts(mut self, fonts: &'static [&'static [u8]]) -> Self {
        self.settings.fonts = Some(fonts);
        self
    }

    /// Register a font family from files on disk.
    ///
    /// Each [`FontFile`](cranpose_ui::text::FontFile) declares the weight and
    /// style its file provides, and a `TextStyle` naming the same family picks
    /// between them. The files are read and parsed here, once, before the app
    /// runs — nothing re-reads them per frame or per string.
    ///
    /// A family whose files cannot be read is reported and skipped; text asking
    /// for it falls back to the default face rather than disappearing.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use cranpose::{
    ///     AppLauncher,
    ///     text::{FontFamily, FontFile, FontWeight},
    /// };
    ///
    /// let roboto = FontFamily::file_backed(vec![
    ///     FontFile::new("/system/fonts/Roboto-Regular.ttf"),
    ///     FontFile::new("/system/fonts/Roboto-Regular.ttf").with_weight(FontWeight::MEDIUM),
    ///     FontFile::new("/system/fonts/Roboto-Regular.ttf").with_weight(FontWeight::BOLD),
    /// ])
    /// .expect("a family needs at least one file");
    ///
    /// let launcher = AppLauncher::new().with_font_family(&roboto);
    /// ```
    pub fn with_font_family(mut self, family: &FontFamily) -> Self {
        if let Err(error) = self.settings.font_registry.register_family(family) {
            log::warn!("font family could not be loaded: {error}");
        }
        self
    }

    /// Register a font family from bytes the app already holds.
    ///
    /// Use this for fonts that are not files on disk — an archive entry, a
    /// download cache, or an asset `cranpose-assets` resolved out of a desktop
    /// bundle (its `load_bytes` returns exactly what this wants). For an APK
    /// asset use `AppLauncher::with_android_asset_font` instead: APK entries
    /// are not filesystem paths, so a path resolver cannot reach them.
    ///
    /// ```no_run
    /// use cranpose::{
    ///     AppLauncher,
    ///     text::{FontFamily, FontStyle, FontWeight},
    /// };
    ///
    /// # fn load(bytes: Vec<u8>) {
    /// let launcher = AppLauncher::new().with_font_face_bytes(
    ///     &FontFamily::named("Roboto"),
    ///     FontWeight::NORMAL,
    ///     FontStyle::Normal,
    ///     bytes,
    /// );
    /// # }
    /// ```
    pub fn with_font_face_bytes(
        mut self,
        family: &FontFamily,
        weight: FontWeight,
        style: FontStyle,
        bytes: impl Into<Vec<u8>>,
    ) -> Self {
        if let Err(error) = self
            .settings
            .font_registry
            .register_face_bytes(family, weight, style, bytes)
        {
            log::warn!("font face could not be loaded: {error}");
        }
        self
    }

    /// Register a font face shipped in the APK's `assets/` directory.
    ///
    /// APK entries are not filesystem paths, so `with_font_family` cannot reach
    /// them; the asset manager the activity already owns can. Call this from
    /// `android_main`, where the `AndroidApp` exists:
    ///
    /// ```no_run
    /// # #[cfg(target_os = "android")]
    /// # fn main(app: android_activity::AndroidApp) {
    /// use cranpose::{
    ///     AppLauncher,
    ///     text::{FontFamily, FontStyle, FontWeight},
    /// };
    ///
    /// let launcher = AppLauncher::new().with_android_asset_font(
    ///     &app,
    ///     &FontFamily::named("Roboto"),
    ///     FontWeight::NORMAL,
    ///     FontStyle::Normal,
    ///     "fonts/Roboto-Regular.ttf",
    /// );
    /// # }
    /// ```
    #[cfg(all(feature = "android", target_os = "android"))]
    pub fn with_android_asset_font(
        mut self,
        app: &android_activity::AndroidApp,
        family: &FontFamily,
        weight: FontWeight,
        style: FontStyle,
        asset_path: &str,
    ) -> Self {
        let Ok(asset_name) = std::ffi::CString::new(asset_path) else {
            log::warn!("asset font path is not a valid C string: {asset_path}");
            return self;
        };
        let Some(mut asset) = app.asset_manager().open(&asset_name) else {
            log::warn!("no font asset at {asset_path}");
            return self;
        };
        if let Err(error) = self
            .settings
            .font_registry
            .register_face_reader(family, weight, style, &mut asset)
        {
            log::warn!("font asset {asset_path} could not be loaded: {error}");
        }
        self
    }

    /// Bind a generic family (`FontFamily::SansSerif`, `Serif`, `Monospace`,
    /// `Cursive`) to the platform's own typeface for it, at Regular, Medium and
    /// Bold.
    ///
    /// Styles keep naming the generic family; they simply stop resolving to the
    /// framework's bundled fallback. On Android this is how an app matches what
    /// Jetpack Compose draws for `FontFamily.SansSerif`, because the platform
    /// backs that alias with its own Roboto.
    ///
    /// `directory` is the platform's font directory —
    /// [`ANDROID_SYSTEM_FONT_DIR`] on Android. If nothing there backs the
    /// family, the failure is reported and the bundled fallback keeps serving.
    pub fn with_system_font_family(
        mut self,
        directory: impl AsRef<Path>,
        family: &FontFamily,
    ) -> Self {
        if let Err(error) = self.settings.font_registry.register_system_family(
            directory,
            family,
            DEFAULT_SYSTEM_FAMILY_WEIGHTS,
        ) {
            log::warn!("system font family could not be loaded: {error}");
        }
        self
    }

    /// Registers a family from the fonts this platform ships, without the
    /// application naming a directory.
    ///
    /// Where a system keeps its fonts is the platform's business, and an
    /// application that spells the path out has target-specific code in it and
    /// draws in the wrong typeface on the target it did not spell out. Every
    /// weight in `weights` is resolved the way the platform resolves it, so a
    /// weight the system has no file for lands on the face it would have
    /// returned rather than being skipped.
    ///
    /// Platforms with no readable font directory — the browser, which has no
    /// filesystem and draws with the fonts the page already has — register
    /// nothing and leave the app on its own faces.
    pub fn with_system_fonts(mut self, family: &FontFamily, weights: &[FontWeight]) -> Self {
        let Some(directory) = crate::system_font_directory() else {
            return self;
        };
        let weights = if weights.is_empty() {
            DEFAULT_SYSTEM_FAMILY_WEIGHTS
        } else {
            weights
        };
        if let Err(error) = self
            .settings
            .font_registry
            .register_system_family(directory, family, weights)
        {
            log::warn!("system font family could not be loaded: {error}");
        }
        self
    }

    /// The tag this application's log lines carry.
    ///
    /// Android routes every line through one tag and `adb logcat -s <tag>` is
    /// how anyone reads them. Naming it here means an application never
    /// initialises a platform logger of its own to get its name onto its lines.
    pub fn with_log_tag(mut self, tag: impl Into<String>) -> Self {
        self.settings.log_tag = Some(tag.into());
        self
    }

    /// Register fonts through the registry directly, for apps that want the
    /// per-face `Result` rather than a logged warning.
    pub fn with_fonts_from(
        mut self,
        register: impl FnOnce(&mut SoftwareTextFontRegistry) -> Result<(), FontLoadError>,
    ) -> Self {
        if let Err(error) = register(&mut self.settings.font_registry) {
            log::warn!("app font registration failed: {error}");
        }
        self
    }

    /// Enable system font loading on Android (default: false).
    ///
    /// When false, only fonts provided via `with_fonts()`, `with_font_family()`
    /// and friends are used. When true, the platform's `sans-serif`, `serif`
    /// and `monospace` faces are registered from
    /// [`ANDROID_SYSTEM_FONT_DIR`] in addition, so styles naming those generic
    /// families draw in the system typeface.
    ///
    /// Android backs those aliases with variable fonts on modern builds; the
    /// registry instances them per weight on their `wght` axis rather than
    /// drawing every weight at the file's default.
    ///
    /// Text that names no family at all also lands on the system face, because
    /// registered faces outrank the plain `with_fonts()` bytes on a tie — the
    /// same thing Compose does, where `FontFamily.Default` is `sans-serif` on
    /// Android. An app that wants its own bundled font for unnamed text should
    /// leave this off and register its family by name instead.
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
    /// #[cfg(all(
    ///     feature = "desktop-shell",
    ///     feature = "renderer-wgpu",
    ///     not(target_os = "android")
    /// ))]
    /// {
    ///     let launcher = AppLauncher::new()
    ///         .with_title("Robot Test")
    ///         .with_size(800, 600)
    ///         .with_headless(true);
    ///
    ///     #[cfg(feature = "robot")]
    ///     let launcher = launcher.with_test_driver(|robot| {
    ///         robot.wait_for_idle().unwrap();
    ///         robot.click(100.0, 100.0).unwrap();
    ///         robot.exit().unwrap();
    ///     });
    ///
    ///     launcher.run(|| {
    ///         // Your composable UI here
    ///     });
    /// }
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
    /// #[cfg(all(
    ///     feature = "desktop-shell",
    ///     feature = "renderer-wgpu",
    ///     not(target_os = "android")
    /// ))]
    /// {
    ///     AppLauncher::new()
    ///         .with_title("My App")
    ///         .with_fps_counter(true)
    ///         .run(|| {
    ///             // Your composable UI here
    ///         });
    /// }
    /// ```
    #[cfg(all(feature = "desktop-shell", feature = "renderer-wgpu"))]
    pub fn with_fps_counter(mut self, enabled: bool) -> Self {
        self.settings.dev_options.fps_counter = enabled;
        self
    }

    /// Set the initial desktop frame pacing mode.
    ///
    /// This controls whether the desktop surface uses vsync or no-vsync presentation and,
    /// for hard caps, limits redraw scheduling to the requested frame rate.
    #[cfg(all(feature = "desktop-shell", feature = "renderer-wgpu"))]
    pub fn with_frame_pacing_mode(mut self, mode: FramePacingMode) -> Self {
        self.settings.frame_pacing_mode = mode;
        self.settings.dev_options.frame_pacing_mode = mode;
        self.settings.frame_pacing_explicit = true;
        self
    }

    /// Set the initial desktop frame pacing mode.
    #[cfg(not(all(feature = "desktop-shell", feature = "renderer-wgpu")))]
    pub fn with_frame_pacing_mode(self, mode: cranpose_app_shell::FramePacingMode) -> Self {
        let _ = mode;
        self
    }

    /// Enable clickable frame pacing controls in the desktop development overlay.
    ///
    /// Enabling the controls also enables the FPS overlay because the controls are rendered
    /// as part of that overlay.
    #[cfg(all(feature = "desktop-shell", feature = "renderer-wgpu"))]
    pub fn with_frame_pacing_controls(mut self, enabled: bool) -> Self {
        self.settings.dev_options.frame_pacing_controls = enabled;
        if enabled {
            self.settings.dev_options.fps_counter = true;
        }
        self
    }

    /// Enable clickable frame pacing controls in the desktop development overlay.
    #[cfg(not(all(feature = "desktop-shell", feature = "renderer-wgpu")))]
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
    /// #[cfg(all(
    ///     feature = "desktop-shell",
    ///     feature = "renderer-wgpu",
    ///     not(target_os = "android")
    /// ))]
    /// {
    ///     AppLauncher::new()
    ///         .with_title("My App")
    ///         .with_fps_counter(true)
    ///         .run(|| {
    ///             // Your composable UI here
    ///         });
    /// }
    /// ```
    #[cfg(not(all(feature = "desktop-shell", feature = "renderer-wgpu")))]
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
    ///     .with_recording(".cranpose-tmp/my_test.rs")
    ///     .run(|| {
    ///         // Interact with the app, then close
    ///         // Recording is saved automatically
    ///     });
    /// ```
    #[cfg(all(feature = "desktop-shell", feature = "renderer-wgpu"))]
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
    #[cfg(all(
        feature = "desktop-shell",
        feature = "robot",
        feature = "renderer-wgpu"
    ))]
    pub fn with_test_driver(mut self, driver: impl FnOnce(crate::Robot) + Send + 'static) -> Self {
        self.settings.test_driver = Some(Box::new(driver));
        // Robot harnesses measure work throughput and high-refresh cadence
        // contracts; lift the vsync cap unless the harness pinned a mode. The
        // slim Vulkan shell presents on demand and carries no frame-pacing
        // controls, so this only applies to the wgpu desktop shell.
        #[cfg(feature = "renderer-wgpu")]
        if !self.settings.frame_pacing_explicit {
            self.settings.frame_pacing_mode = FramePacingMode::NoVsync;
            self.settings.dev_options.frame_pacing_mode = FramePacingMode::NoVsync;
        }
        self
    }

    #[cfg(all(
        feature = "desktop-shell",
        feature = "robot",
        feature = "renderer-wgpu"
    ))]
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
        feature = "desktop-shell",
        feature = "renderer-wgpu",
        not(target_os = "android")
    ))]
    pub fn try_run(self, content: impl FnMut() + 'static) -> Result<(), LaunchError> {
        crate::desktop::try_run(self.settings, content)
    }

    /// Run the application (Desktop platform).
    ///
    /// Use [`AppLauncher::try_run`] when the caller needs a typed launch failure.
    #[cfg(all(
        feature = "desktop-shell",
        feature = "renderer-wgpu",
        not(target_os = "android")
    ))]
    pub fn run(self, content: impl FnMut() + 'static) -> ! {
        self.try_run(content)
            .unwrap_or_else(|error| exit_after_launch_error("desktop launch failed", error));
        std::process::exit(0)
    }

    /// Run a desktop app that declares its visible operating-system windows directly.
    ///
    /// The primary launcher surface is kept hidden; content should declare peer
    /// windows with `WindowNode` or `Window`.
    #[cfg(all(
        feature = "desktop-shell",
        feature = "renderer-wgpu",
        not(target_os = "android")
    ))]
    pub fn try_run_windows(mut self, content: impl FnMut() + 'static) -> Result<(), LaunchError> {
        self.settings.primary_window_visible = false;
        self.try_run(content)
    }

    /// Run a desktop app that declares its visible operating-system windows directly.
    #[cfg(all(
        feature = "desktop-shell",
        feature = "renderer-wgpu",
        not(target_os = "android")
    ))]
    pub fn run_windows(self, content: impl FnMut() + 'static) -> ! {
        self.try_run_windows(content)
            .unwrap_or_else(|error| exit_after_launch_error("desktop launch failed", error));
        std::process::exit(0)
    }

    /// Run the application (iOS platform).
    ///
    /// Drives winit's UIKit event loop and blocks for the lifetime of the app.
    /// `UIApplicationMain` is started by winit, so the iOS app binary needs no
    /// Objective-C entry point.
    ///
    /// # Arguments
    ///
    /// * `content` - The root composable function of your application.
    #[cfg(all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"))]
    pub fn try_run(self, content: impl FnMut() + 'static) -> Result<(), LaunchError> {
        crate::ios::try_run(self.settings, content)
    }

    /// Run the application (iOS platform).
    ///
    /// Use [`AppLauncher::try_run`] when the caller needs a typed launch failure.
    #[cfg(all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"))]
    pub fn run(self, content: impl FnMut() + 'static) -> ! {
        self.try_run(content)
            .unwrap_or_else(|error| exit_after_launch_error("iOS launch failed", error));
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
        crate::android::run(app, self.settings, content);
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
        crate::web::run(canvas_id, self.settings, content).await
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
    fn a_launcher_records_the_application_id_it_is_given() {
        let launcher = AppLauncher::new();
        assert!(
            launcher.settings.application_id.is_none(),
            "an id appeared without being stated"
        );

        let launcher = AppLauncher::new().with_application_id("com.example.notes");
        assert_eq!(
            launcher.settings.application_id.as_deref(),
            Some("com.example.notes")
        );
    }

    #[test]
    fn a_launcher_records_the_android_system_font_choice() {
        assert!(
            !AppLauncher::new().settings.android_use_system_fonts,
            "system fonts must be opt-in: loading them costs a scan of /system/fonts"
        );
        assert!(
            AppLauncher::new()
                .with_android_use_system_fonts(true)
                .settings
                .android_use_system_fonts
        );
    }

    #[test]
    fn a_launcher_records_the_overlay_window_it_is_given() {
        assert!(AppLauncher::new().settings.android_overlay_window.is_none());

        let options = AndroidOverlayWindowOptions::new(320, 180);
        let launcher = AppLauncher::new().with_android_overlay_window(options);
        let stored = launcher
            .settings
            .android_overlay_window
            .expect("the overlay options were dropped");
        assert_eq!((stored.width, stored.height), (320, 180));
    }

    #[test]
    fn a_font_registration_closure_is_run_against_the_launchers_own_registry() {
        use std::{cell::Cell, rc::Rc};

        let ran = Rc::new(Cell::new(false));
        let flag = Rc::clone(&ran);
        let _launcher = AppLauncher::new().with_fonts_from(move |_registry| {
            flag.set(true);
            Ok(())
        });
        assert!(ran.get(), "the registration closure never ran");
    }

    #[test]
    fn a_font_registration_that_fails_leaves_a_usable_launcher() {
        // A missing or unreadable font is a warning, not a launch failure: an
        // app that cannot find one face still has to start and draw with the
        // fallbacks.
        let launcher = AppLauncher::new()
            .with_fonts_from(|_registry| Err(FontLoadError::EmptyFamily))
            .with_title("still here");
        assert_eq!(launcher.settings.window_title, "still here");
    }

    #[test]
    fn face_bytes_that_are_not_a_font_do_not_stop_the_launcher() {
        let launcher = AppLauncher::new()
            .with_font_face_bytes(
                &FontFamily::SansSerif,
                FontWeight::NORMAL,
                FontStyle::Normal,
                b"not a font".to_vec(),
            )
            .with_title("still here");
        assert_eq!(launcher.settings.window_title, "still here");
    }

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

    #[cfg(all(feature = "desktop-shell", feature = "renderer-wgpu"))]
    #[test]
    fn production_apps_default_to_vsync_frame_pacing() {
        // A desktop UI app must not render animations uncapped (hundreds of fps
        // on a 60Hz panel saturates the GPU and ruins scroll latency). Vsync is
        // the production default; harnesses opt out explicitly.
        assert_eq!(
            AppSettings::default().frame_pacing_mode,
            FramePacingMode::Vsync
        );
        assert_eq!(FramePacingMode::default(), FramePacingMode::Vsync);
    }

    #[cfg(all(
        feature = "desktop-shell",
        feature = "renderer-wgpu",
        feature = "robot"
    ))]
    #[test]
    fn robot_test_driver_defaults_to_uncapped_frame_pacing() {
        // Robot/perf harnesses measure work throughput and 120Hz cadence
        // contracts; installing a test driver lifts the vsync cap unless the
        // harness chose a pacing mode explicitly.
        let launcher = AppLauncher::new().with_test_driver(|_| {});
        assert_eq!(
            launcher.settings.frame_pacing_mode,
            FramePacingMode::NoVsync
        );

        let pinned = AppLauncher::new()
            .with_frame_pacing_mode(FramePacingMode::Hard60)
            .with_test_driver(|_| {});
        assert_eq!(pinned.settings.frame_pacing_mode, FramePacingMode::Hard60);

        let pinned_after = AppLauncher::new()
            .with_test_driver(|_| {})
            .with_frame_pacing_mode(FramePacingMode::Vsync);
        assert_eq!(
            pinned_after.settings.frame_pacing_mode,
            FramePacingMode::Vsync
        );
    }
}
