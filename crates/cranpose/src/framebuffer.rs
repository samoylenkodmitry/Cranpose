//! A composition that the CPU draws into an RGBA framebuffer, for hosts
//! without a GPU: the watchOS host and microcontroller hosts such as
//! `cranpose-platform-esp-idf`.
//!
//! The host owns the display. It feeds the frame clock, touches and lifecycle
//! changes into a [`Framebuffer`]. When [`Framebuffer::tick`] reports a new
//! picture, [`Framebuffer::changed_areas`] lists the parts of the display it
//! changes and [`Framebuffer::draw_area`] draws each of them, so the CPU draws
//! and the host sends only the pixels that change.

use std::rc::Rc;

pub use cranpose_app_shell::FrameSchedule;
use cranpose_app_shell::{AppShell, default_root_key};
use cranpose_foundation::PointerSource;
use cranpose_render_pixels::{Picture, PixelsRenderer};
pub use cranpose_render_pixels::{PixelRect, Rgb565Order};
use cranpose_services::{
    SystemTheme,
    host::{LifecycleState, dispatch_lifecycle_state},
};
use cranpose_ui::EdgeInsets;

use crate::platform_env::PlatformEnvironment;

/// An invalid framebuffer configuration.
#[derive(Debug, thiserror::Error)]
#[error(
    "a framebuffer needs nonzero dimensions, a finite positive density, and addressable RGBA storage"
)]
pub struct SurfaceError;

/// The phase of one touch sample.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TouchPhase {
    /// The finger touched the display.
    Down,
    /// The finger moved while touching the display.
    Move,
    /// The finger left the display.
    Up,
    /// The system took the touch away.
    Cancel,
}

/// One composition that the CPU draws, area by area, for a display the host
/// owns.
///
/// Create and use it on one thread, which becomes the UI thread.
pub struct Framebuffer {
    shell: AppShell<PixelsRenderer>,
    drawing: Picture,
    width: u32,
    height: u32,
    density: f32,
    active: bool,
    needs_present: bool,
    picture: u64,
    drawn_picture: Option<u64>,
    environment: Rc<PlatformEnvironment>,
}

pub(crate) fn frame_bytes(width: u32, height: u32, density: f32) -> Result<usize, SurfaceError> {
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

impl Framebuffer {
    // Out of line: inlined, the renderer and shell it builds kept their room
    // on the host's stack under every frame its loop runs.
    /// Composes `content` for a display of `width` by `height` physical
    /// pixels. `density` is physical pixels per point, and `theme` is the
    /// system theme content reads. Text draws in `fonts`; with none, in the
    /// embedded default font of the `embedded-default-font` feature.
    ///
    /// Frames start once the host calls [`Self::set_active`].
    #[inline(never)]
    pub fn new(
        width: u32,
        height: u32,
        density: f32,
        theme: SystemTheme,
        fonts: &[&'static [u8]],
        mut content: impl FnMut() + 'static,
    ) -> Result<Self, SurfaceError> {
        frame_bytes(width, height, density)?;
        let environment = PlatformEnvironment::new();
        environment.set_system_theme(theme);
        #[cfg(feature = "watchos")]
        environment.set_configuration(cranpose_ui::Configuration {
            ui_mode_type: cranpose_ui::UiModeType::Watch,
        });
        let root_environment = Rc::clone(&environment);
        let mut shell = AppShell::new_with_size_and_density(
            PixelsRenderer::with_fonts(fonts),
            default_root_key(),
            move || root_environment.compose_root(&mut content),
            (width, height),
            (width as f32 / density, height as f32 / density),
            density,
        );
        shell.set_pointer_source(PointerSource::Touch);
        shell.set_cursor(width as f32 / density / 2.0, height as f32 / density / 2.0);
        Ok(Self {
            shell,
            drawing: Picture::default(),
            width,
            height,
            density,
            active: false,
            needs_present: true,
            picture: 0,
            drawn_picture: None,
            environment,
        })
    }

    /// Applies the display's safe area in points: the edges the system draws
    /// on. Content reads it from `local_safe_area_insets`.
    pub fn set_safe_area(&mut self, insets: EdgeInsets) {
        self.shell.set_safe_area(insets);
        if self.environment.set_safe_area(insets) {
            self.shell.request_root_render();
        }
    }

    /// Applies a new physical display size without replacing the composition.
    pub fn resize(&mut self, width: u32, height: u32, density: f32) -> Result<(), SurfaceError> {
        frame_bytes(width, height, density)?;
        if (width, height, density) == (self.width, self.height, self.density) {
            return Ok(());
        }
        self.width = width;
        self.height = height;
        self.density = density;
        self.shell.set_density(density);
        let mut surface = self.shell.primary();
        surface.set_buffer_size(width, height);
        surface.set_viewport(width as f32 / density, height as f32 / density);
        self.needs_present = true;
        self.picture = self.picture.wrapping_add(1);
        Ok(())
    }

    /// Advances the frame clock to `elapsed_nanos` since launch. Returns
    /// whether the picture changed, so the host should present a new frame.
    pub fn tick(&mut self, elapsed_nanos: u64) -> bool {
        if !self.active {
            return false;
        }
        let result = self.shell.update_at_frame_time_nanos(elapsed_nanos);
        if !self.needs_present && !result.visual_changed {
            return false;
        }
        self.needs_present = false;
        self.picture = self.picture.wrapping_add(1);
        true
    }

    /// The work the composition has for the host: whether it needs an
    /// update, whether it needs a frame, and the time of its next timed
    /// event. A host sleeps until then when it needs neither.
    pub fn frame_schedule(&self) -> FrameSchedule {
        self.shell.frame_schedule()
    }

    /// Calls `waker` from any thread when work arrives for the UI thread,
    /// such as a coroutine's continuation, so a sleeping host wakes for it.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_frame_waker(&mut self, waker: impl Fn() + Send + Sync + 'static) {
        self.shell.set_frame_waker(waker);
    }

    /// Runs the tasks woken since they last ran. A host calls this whenever
    /// its loop wakes.
    pub fn run_pending_tasks(&mut self) {
        self.shell.run_pending_tasks();
    }

    /// Takes the current picture and puts into `areas` the parts of the
    /// display whose pixels differ from the picture taken before: the whole
    /// display the first time and after a resize, none while the picture stays
    /// the same. The host draws them with [`Self::draw_area`].
    pub fn changed_areas(&mut self, areas: &mut Vec<PixelRect>) {
        if self.drawn_picture == Some(self.picture) {
            areas.clear();
            return;
        }
        let (width, height, density) = (self.width, self.height, self.density);
        let drawing = &mut self.drawing;
        self.shell.primary().with_renderer(|renderer| {
            renderer.update_picture(drawing, width, height, density, areas);
        });
        self.drawn_picture = Some(self.picture);
    }

    /// Rasterizes the glyphs of the text in `area` of the picture
    /// [`Self::changed_areas`] took last, so that [`Self::draw_area`] takes
    /// them from the glyph cache. A host calls it while it waits anyway, such
    /// as for a display that is still starting up.
    pub fn prepare_area(&mut self, area: PixelRect) {
        let drawing = &self.drawing;
        self.shell.primary().with_renderer(|renderer| {
            renderer.prepare_picture_area(drawing, area);
        });
    }

    /// Draws `area` of the picture [`Self::changed_areas`] took last into
    /// `pixels`, as row-major, straight-alpha RGBA8 rows of `area.width`
    /// pixels, `area.height` of them.
    pub fn draw_area(&mut self, area: PixelRect, pixels: &mut [u8]) {
        let drawing = &self.drawing;
        self.shell.primary().with_renderer(|renderer| {
            renderer.draw_picture_area(drawing, area, pixels);
        });
    }

    /// Draws `area` of the picture [`Self::changed_areas`] took last into
    /// `pixels` as a display of 16-bit pixels takes them: 5 bits of red, 6 of
    /// green and 5 of blue, stored in `order`, rows of `area.width` pixels,
    /// `area.height` of them. It draws them straight, without the bytes
    /// [`Self::draw_area`] draws and a second pass over them.
    pub fn draw_area_rgb565(&mut self, area: PixelRect, pixels: &mut [u16], order: Rgb565Order) {
        let drawing = &self.drawing;
        self.shell.primary().with_renderer(|renderer| {
            renderer.draw_picture_area_rgb565(drawing, area, pixels, order);
        });
    }

    /// Returns the physical image width.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Returns the physical image height.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Routes one touch sample at `x`, `y` in display points. Non-finite
    /// coordinates are ignored.
    pub fn touch(&mut self, phase: TouchPhase, x: f32, y: f32) {
        if !self.active || !x.is_finite() || !y.is_finite() {
            return;
        }
        match phase {
            TouchPhase::Down => {
                self.shell.set_cursor(x, y);
                self.shell.pointer_pressed();
            }
            TouchPhase::Move => {
                self.shell.set_cursor(x, y);
            }
            TouchPhase::Up => {
                self.shell.pointer_released_at_position(x, y);
            }
            TouchPhase::Cancel => self.shell.cancel_gesture(),
        }
    }

    /// Routes rotary input in detents through Cranpose's detent-to-scroll
    /// conversion. `elapsed_millis` is the time since launch. Returns whether
    /// a scrollable took the input.
    pub fn rotary(&mut self, delta: f32, elapsed_millis: u64) -> bool {
        if !self.active || !delta.is_finite() {
            return false;
        }
        self.shell.rotary_scrolled_by_detents(delta, elapsed_millis)
    }

    /// Suspends frame production and cancels held touches when the display
    /// leaves the foreground, and resumes it when it returns.
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

    #[cfg(feature = "watchos")]
    pub(crate) fn picture(&self) -> u64 {
        self.picture
    }

    #[cfg(feature = "watchos")]
    pub(crate) fn with_renderer<T>(
        &mut self,
        draw: impl FnOnce(&PixelsRenderer, u32, u32, f32) -> T,
    ) -> T {
        let (width, height, density) = (self.width, self.height, self.density);
        self.shell
            .primary()
            .with_renderer(|renderer| draw(renderer, width, height, density))
    }
}
