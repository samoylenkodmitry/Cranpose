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
use cranpose_ui::{
    ImageBitmap,
    text::{FontFamily, FontStyle, FontWeight},
};
#[cfg(all(
    feature = "renderer-wgpu",
    any(feature = "desktop-shell", all(feature = "ios", target_os = "ios"))
))]
use thiserror::Error;

/// Graphics API used by the Android WGPU renderer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AndroidGpuBackend {
    /// Render through Vulkan.
    #[default]
    Vulkan,
    /// Render through OpenGL ES.
    OpenGlEs,
}

#[cfg(any(
    test,
    all(feature = "android", feature = "renderer-wgpu", target_os = "android")
))]
impl AndroidGpuBackend {
    pub(crate) fn with_override(self, value: Option<&str>) -> Self {
        match value.map(str::trim) {
            Some(value) if value.eq_ignore_ascii_case("vulkan") => Self::Vulkan,
            Some(value)
                if ["gl", "gles", "opengl"]
                    .iter()
                    .any(|name| value.eq_ignore_ascii_case(name)) =>
            {
                Self::OpenGlEs
            }
            _ => self,
        }
    }
}

#[cfg(test)]
#[path = "tests/inspector_settings.rs"]
mod inspector_settings_tests;

#[cfg(test)]
#[path = "tests/window_icon_settings.rs"]
mod window_icon_settings_tests;

#[cfg(test)]
#[path = "tests/custom_cursor_size_settings.rs"]
mod custom_cursor_size_settings_tests;

/// How big an app's own cursor images appear when the person has enlarged the
/// system pointer.
///
/// Platforms disagree here. macOS enlarges every cursor by its accessibility
/// pointer size, an app's own images included, and so do browsers on it, which
/// hand a CSS cursor image to the system. Windows, X11 and Wayland enlarge
/// only their own cursors and show an app's image at its pixel size. Cranpose
/// evens this out on every platform, desktop and web: a custom cursor image is
/// taken as drawn one pixel to a logical point, and appears at the size chosen
/// here. Standard cursors follow the system whichever is chosen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CustomCursorSize {
    /// Custom cursors grow with the system pointer, as the standard ones do.
    /// The default: the person enlarged the pointer to see it.
    #[default]
    FollowSystem,
    /// Custom cursors keep the size they were drawn at, every pixel of the
    /// image intact, however the system pointer is set.
    AsDrawn,
}

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
    /// What this application declared it asks of a device.
    ///
    /// The same declaration the platform builds read, so a service can answer
    /// on every platform without each one keeping its own list.
    pub capabilities: cranpose_capabilities::Capabilities<'static>,
    /// Desktop only: the picture every window the application opens carries.
    ///
    /// Windows shows it in the title bar, the taskbar and the task switcher,
    /// and Linux desktops show it in their panels and switchers. macOS,
    /// Android, iOS and the web take the application's icon from what the
    /// platform packaged and ignore this field.
    pub window_icon: Option<ImageBitmap>,
    /// Initial window width in logical pixels.
    pub initial_width: u32,
    /// Initial window height in logical pixels.
    pub initial_height: u32,
    /// Whether the initial size was explicitly supplied by the app.
    pub initial_size_explicit: bool,
    /// Desktop only: keep the primary window the size of what it lays out.
    pub primary_wraps_content: bool,
    /// Web only: give the canvas the full browser viewport instead of the box
    /// the host page lays it out in.
    ///
    /// The default leaves the canvas box to the page's own stylesheet, so a
    /// canvas nested in a phone frame, a card or any clipping container keeps
    /// the size that container gives it. An app that owns the whole page opts
    /// in here and the canvas fills the dynamic viewport, tracking the browser
    /// window as it resizes; other platforms ignore this field.
    pub web_fill_viewport: bool,
    /// Desktop and web: how big an app's own cursor images appear when the
    /// person has enlarged the system pointer.
    ///
    /// Left at [`CustomCursorSize::FollowSystem`], they grow with it, as the
    /// standard cursors do. An app whose cursors are pixel art drawn at an exact
    /// size can ask for [`CustomCursorSize::AsDrawn`] instead; on the web the
    /// page then draws such a cursor itself, since a browser leaves the size
    /// of a CSS cursor to the system.
    pub custom_cursor_size: CustomCursorSize,
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
    /// Graphics API selected for the Android WGPU renderer.
    pub android_gpu_backend: AndroidGpuBackend,
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
    /// Override the developer inspector default. Unset enables it in debug builds;
    /// installing a robot driver defaults it to disabled unless explicitly set.
    pub developer_inspector: Option<bool>,
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
            capabilities: cranpose_capabilities::Capabilities::NONE,
            window_icon: None,
            initial_width: 800,
            initial_height: 600,
            initial_size_explicit: false,
            primary_wraps_content: false,
            web_fill_viewport: false,
            custom_cursor_size: CustomCursorSize::FollowSystem,
            fonts: None,
            font_registry: SoftwareTextFontRegistry::new(),
            android_use_system_fonts: false,
            android_gpu_backend: AndroidGpuBackend::default(),
            log_tag: None,
            android_overlay_window: None,
            headless: false,
            developer_inspector: None,
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

    /// State what this application asks of a device.
    ///
    /// The value comes from the build script, through
    /// [`app_capabilities!`](crate::app_capabilities):
    ///
    /// ```ignore
    /// cranpose::app_capabilities!();
    ///
    /// AppLauncher::new().with_capabilities(&CAPABILITIES)
    /// ```
    ///
    /// The platform builds read the same declaration, so the permissions in
    /// the manifest and the answers a service gives at run time cannot drift
    /// apart.
    #[must_use]
    pub fn with_capabilities(
        mut self,
        capabilities: &cranpose_capabilities::Capabilities<'static>,
    ) -> Self {
        self.settings.capabilities = *capabilities;
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
    /// leave uncovered (black) strips of display around the surface. Web
    /// ignores it: the canvas takes the box the host page lays it out in, or
    /// the viewport under [`Self::with_web_fill_viewport`].
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

    /// Desktop only: keep the primary window the size of what it lays out,
    /// measured after every update, so a window that holds a stack of panes
    /// is exactly that stack and shrinks when a pane leaves for a window of
    /// its own. The window keeps its top-left corner as it resizes. The
    /// initial size applies until the first layout; other platforms ignore
    /// this.
    pub fn with_window_wrapping_content(mut self) -> Self {
        self.settings.primary_wraps_content = true;
        self
    }

    /// Web only: size the canvas to the full browser viewport and keep it
    /// tracking that viewport's size as the window resizes, instead of taking
    /// the box the host page's stylesheet lays the canvas out in. Other
    /// platforms ignore this.
    pub fn with_web_fill_viewport(mut self, fill: bool) -> Self {
        self.settings.web_fill_viewport = fill;
        self
    }

    /// Desktop and web: how big the app's own cursor images appear when the
    /// person has enlarged the system pointer. See [`CustomCursorSize`].
    pub fn with_custom_cursor_size(mut self, size: CustomCursorSize) -> Self {
        self.settings.custom_cursor_size = size;
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

    /// Desktop only: the picture every window the application opens carries,
    /// in the title bar, taskbar and task switcher of Windows and Linux.
    ///
    /// macOS, Android, iOS and the web take the application's icon from what
    /// the platform packaged and ignore it. A square picture of 64 to 256
    /// pixels a side reads well at every size those desktops draw it.
    pub fn with_window_icon(mut self, icon: ImageBitmap) -> Self {
        self.settings.window_icon = Some(icon);
        self
    }

    /// Enables the developer inspector independently of application semantics.
    ///
    /// Enabled by default in debug builds and disabled in release builds.
    /// Robot drivers default to disabled; this override wins in either builder order.
    /// Open it with the floating Inspector control; drag the control or panel title to move it.
    pub fn with_developer_inspector(mut self, enabled: bool) -> Self {
        self.settings.developer_inspector = Some(enabled);
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

    /// Selects the Android graphics API. Other platforms ignore this setting.
    ///
    /// The default is Vulkan. OpenGL ES uses the same WGPU renderer through
    /// the device's GLES driver. Profiling overrides can replace this choice.
    pub fn with_android_gpu_backend(mut self, backend: AndroidGpuBackend) -> Self {
        self.settings.android_gpu_backend = backend;
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
        self.settings.developer_inspector.get_or_insert(false);
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

    /// Run the application inside the host program that started it.
    ///
    /// Connects to `endpoint`, then draws every frame off screen and streams
    /// it to the host, which sends size, input and theme events back. The call
    /// returns when the host closes the connection. See [`crate::embed`].
    #[cfg(feature = "embed")]
    pub fn try_run_embedded(
        self,
        endpoint: crate::embed::EmbedEndpoint,
        content: impl FnMut() + 'static,
    ) -> Result<(), crate::embed::EmbedError> {
        crate::embed::try_run(self.settings, endpoint, content)
    }

    /// Run the application inside the host program that started it, and exit
    /// the process when the host closes it.
    ///
    /// Use [`AppLauncher::try_run_embedded`] when the caller needs a typed
    /// failure.
    #[cfg(feature = "embed")]
    pub fn run_embedded(
        self,
        endpoint: crate::embed::EmbedEndpoint,
        content: impl FnMut() + 'static,
    ) -> ! {
        match self.try_run_embedded(endpoint, content) {
            Ok(()) => std::process::exit(0),
            Err(error) => {
                eprintln!("embedded launch failed: {error}");
                std::process::exit(1)
            }
        }
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
    #[cfg(all(feature = "android", feature = "renderer-wgpu", target_os = "android"))]
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
#[path = "tests/app_launcher_tests.rs"]
mod tests;
