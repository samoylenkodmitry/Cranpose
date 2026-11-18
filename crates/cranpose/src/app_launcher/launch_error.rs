//! What a failed launch of a windowed application reports.

#[cfg(all(
    feature = "renderer-wgpu",
    any(feature = "desktop-shell", all(feature = "ios", target_os = "ios"))
))]
use thiserror::Error;

/// Errors that can occur while launching a windowed (desktop or iOS) application.
#[cfg(all(
    feature = "renderer-wgpu",
    any(feature = "desktop-shell", all(feature = "ios", target_os = "ios"))
))]
#[derive(Debug, Error)]
pub enum LaunchError {
    /// Connecting to or rendering inside an embedding host failed.
    #[cfg(feature = "embed")]
    #[error("embedded launch failed: {0}")]
    Embedded(#[from] crate::embed::EmbedError),
    /// Creating the desktop event loop failed.
    #[error("failed to create desktop event loop: {0}")]
    EventLoopCreate(#[source] winit::error::EventLoopError),
    /// Creating the desktop window failed.
    #[error("failed to create desktop window: {0}")]
    WindowCreate(#[source] winit::error::CreateWindowError),
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
    /// The GPU device was lost, and the devices opened in its place were
    /// lost before they presented a frame.
    #[error("the GPU device was lost and could not be replaced")]
    GpuDeviceLost,
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
#[expect(clippy::print_stderr, reason = "a failed launch is reported")]
pub(crate) fn exit_after_launch_error(context: &str, error: &LaunchError) -> ! {
    eprintln!("{context}: {error}");
    std::process::exit(1)
}
