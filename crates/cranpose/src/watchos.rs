//! Experimental watchOS host runtime, using Cranpose's software renderer.
//!
//! The native host owns presentation and feeds physical surface sizes, touch
//! positions in points, crown deltas, and lifecycle changes into this runtime.
//! Application content remains an ordinary Rust composable. No wgpu or winit
//! dependency is required. The host installs the application's folders, taps,
//! the microphone permission and, with the `wearable` feature, the link to the
//! iPhone app. Audio playback, purchases, and accessibility are not yet
//! implemented by this experimental host.

use cranpose_app_shell::{AppShell, default_root_key};
use cranpose_foundation::PointerSource;
use cranpose_render_pixels::PixelsRenderer;
use cranpose_services::host::{LifecycleState, dispatch_lifecycle_state};

/// An invalid watch display configuration.
#[derive(Debug, thiserror::Error)]
#[error(
    "watch surface needs nonzero dimensions, a finite positive density, and addressable RGBA storage"
)]
pub struct SurfaceError;

/// One watch UI composition and its retained RGBA framebuffer.
///
/// Create and use this object on the native host's main thread. The framebuffer
/// is borrowed until the next mutable call; a presenter must retain its own
/// image storage when the operating system displays frames asynchronously.
pub struct Application {
    shell: AppShell<PixelsRenderer>,
    pixels: Vec<u8>,
    width: u32,
    height: u32,
    density: f32,
    active: bool,
    needs_present: bool,
}

fn buffer_length(width: u32, height: u32, density: f32) -> Result<usize, SurfaceError> {
    if width == 0 || height == 0 || !density.is_finite() || density <= 0.0 {
        return Err(SurfaceError);
    }
    usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4))
        .filter(|bytes| *bytes <= isize::MAX as usize)
        .ok_or(SurfaceError)
}

impl Application {
    /// Composes shared application content at the display's physical resolution.
    /// `density` is physical pixels per point, as reported by the watch.
    /// The host calls [`Self::set_active`] when the scene enters the foreground.
    pub fn new(
        width: u32,
        height: u32,
        density: f32,
        content: impl FnMut() + 'static,
    ) -> Result<Self, SurfaceError> {
        let len = buffer_length(width, height, density)?;
        #[cfg(target_os = "watchos")]
        crate::watchos_services::register();
        let mut pixels = Vec::new();
        pixels.try_reserve_exact(len).map_err(|_| SurfaceError)?;
        pixels.resize(len, 0);
        let mut shell = AppShell::new_with_size_and_density(
            PixelsRenderer::new(),
            default_root_key(),
            content,
            (width, height),
            (width as f32 / density, height as f32 / density),
            density,
        );
        shell.set_pointer_source(PointerSource::Touch);
        shell.set_cursor(width as f32 / density / 2.0, height as f32 / density / 2.0);
        Ok(Self {
            shell,
            pixels,
            width,
            height,
            density,
            active: false,
            needs_present: true,
        })
    }

    /// Applies a new physical surface size without replacing the composition.
    pub fn resize(&mut self, width: u32, height: u32, density: f32) -> Result<(), SurfaceError> {
        let len = buffer_length(width, height, density)?;
        if (width, height, density) == (self.width, self.height, self.density) {
            return Ok(());
        }
        if len > self.pixels.len() {
            self.pixels
                .try_reserve_exact(len - self.pixels.len())
                .map_err(|_| SurfaceError)?;
        }
        self.pixels.resize(len, 0);
        self.width = width;
        self.height = height;
        self.density = density;
        self.shell.set_density(density);
        let mut surface = self.shell.primary();
        surface.set_buffer_size(width, height);
        surface.set_viewport(width as f32 / density, height as f32 / density);
        self.needs_present = true;
        Ok(())
    }

    /// Advances the shared frame clock, rasterizing only when the picture changes.
    /// Returns whether a new image should be presented.
    pub fn tick(&mut self, elapsed_nanos: u64) -> bool {
        if !self.active {
            return false;
        }
        let result = self.shell.update_at_frame_time_nanos(elapsed_nanos);
        if !self.needs_present && !result.visual_changed {
            return false;
        }
        self.shell.primary().with_renderer(|renderer| {
            renderer.draw_scaled(&mut self.pixels, self.width, self.height, self.density);
        });
        self.needs_present = false;
        true
    }

    /// Borrows the latest row-major, straight-alpha RGBA8 image.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// Returns the physical image width.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Returns the physical image height.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Routes one touch sample in display points: down=0, move=1, up=2, cancel=3.
    /// Invalid phases and non-finite coordinates are ignored.
    pub fn touch(&mut self, phase: u8, x: f32, y: f32) {
        if !self.active || phase > 3 || !x.is_finite() || !y.is_finite() {
            return;
        }
        match phase {
            0 => {
                self.shell.set_cursor(x, y);
                self.shell.pointer_pressed();
            }
            1 => {
                self.shell.set_cursor(x, y);
            }
            2 => {
                self.shell.pointer_released_at_position(x, y);
            }
            _ => self.shell.cancel_gesture(),
        }
    }

    /// Routes crown units through Cranpose's existing detent-to-scroll conversion.
    /// The native host supplies the timestamp in milliseconds since launch.
    pub fn crown(&mut self, delta: f32, elapsed_millis: u64) -> bool {
        if !self.active || !delta.is_finite() {
            return false;
        }
        self.shell.rotary_scrolled_by_detents(delta, elapsed_millis)
    }

    /// Suspends frame production and cancels held touches when leaving the foreground.
    pub fn set_active(&mut self, active: bool) {
        if self.active == active {
            return;
        }
        self.active = active;
        if active {
            self.needs_present = true;
            dispatch_lifecycle_state(LifecycleState::Resumed);
        } else {
            self.shell.cancel_gesture();
            dispatch_lifecycle_state(LifecycleState::Paused);
        }
    }
}
