//! Experimental watchOS host runtime.
//!
//! watchOS gives an app no Metal of its own, so the host draws with
//! SpriteKit: each frame the runtime hands it the scene as sprites
//! ([`Application::sprites`]), and SpriteKit puts them on the GPU. The same
//! scene rasterizes on the CPU as well ([`Application::pixels`]), which is
//! what snapshots and tests read.
//!
//! The native host owns presentation and feeds physical surface sizes, touch
//! positions in points, crown deltas, and lifecycle changes into this runtime.
//! Application content remains an ordinary Rust composable. No wgpu or winit
//! dependency is required. The host installs the application's folders, taps,
//! the microphone permission and, with the `wearable` feature, the link to the
//! iPhone app. Audio playback, purchases, and accessibility are not yet
//! implemented by this experimental host.

pub use cranpose_render_pixels::sprites::{SPRITE_FLOATS, SpriteFrame, TextureUpload, field};
use cranpose_services::SystemTheme;
use cranpose_ui::EdgeInsets;

pub use crate::framebuffer::SurfaceError;
use crate::framebuffer::{Framebuffer, TouchPhase};

/// One watch UI composition, drawn as sprites or into a retained RGBA
/// framebuffer on request.
///
/// Create and use this object on the native host's main thread.
pub struct Application {
    framebuffer: Framebuffer,
    sprites: SpriteFrame,
    sprites_picture: Option<u64>,
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
        #[cfg(target_os = "watchos")]
        crate::watchos_services::register();
        // A watch shows its apps on black.
        let framebuffer = Framebuffer::new(width, height, density, SystemTheme::Dark, content)?;
        Ok(Self {
            framebuffer,
            sprites: SpriteFrame::default(),
            sprites_picture: None,
        })
    }

    /// Applies the screen's safe area in points: the edges the system draws
    /// on, such as the clock. Content reads it from `local_safe_area_insets`.
    /// Negative and non-finite edges count as none.
    pub fn set_safe_area(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        let edge = |value: f32| {
            if value.is_finite() {
                value.max(0.0)
            } else {
                0.0
            }
        };
        self.framebuffer.set_safe_area(EdgeInsets::from_components(
            edge(left),
            edge(top),
            edge(right),
            edge(bottom),
        ));
    }

    /// Applies a new physical surface size without replacing the composition.
    pub fn resize(&mut self, width: u32, height: u32, density: f32) -> Result<(), SurfaceError> {
        self.framebuffer.resize(width, height, density)
    }

    /// Advances the shared frame clock. Returns whether the picture changed,
    /// so a new frame should be presented.
    pub fn tick(&mut self, elapsed_nanos: u64) -> bool {
        self.framebuffer.tick(elapsed_nanos)
    }

    /// The current picture as a row-major, straight-alpha RGBA8 image,
    /// rasterized on the CPU when it changed.
    pub fn pixels(&mut self) -> &[u8] {
        self.framebuffer.pixels()
    }

    /// The sprites [`Self::sprites`] composed last, without composing.
    pub fn last_sprites(&self) -> &SpriteFrame {
        &self.sprites
    }

    /// The current picture as sprites for SpriteKit, with the textures it
    /// first uses and those it no longer does; see
    /// [`cranpose_render_pixels::sprites`]. Composed when the picture
    /// changed; until it changes again, the same frame, uploads included.
    pub fn sprites(&mut self) -> &SpriteFrame {
        let picture = self.framebuffer.picture();
        if self.sprites_picture != Some(picture) {
            let sprites = &mut self.sprites;
            self.framebuffer
                .with_renderer(|renderer, width, height, density| {
                    renderer.compose_sprites(sprites, width, height, density);
                });
            self.sprites_picture = Some(picture);
        }
        &self.sprites
    }

    /// Returns the physical image width.
    pub fn width(&self) -> u32 {
        self.framebuffer.width()
    }

    /// Returns the physical image height.
    pub fn height(&self) -> u32 {
        self.framebuffer.height()
    }

    /// Routes one touch sample in display points: down=0, move=1, up=2, cancel=3.
    /// Invalid phases and non-finite coordinates are ignored.
    pub fn touch(&mut self, phase: u8, x: f32, y: f32) {
        let phase = match phase {
            0 => TouchPhase::Down,
            1 => TouchPhase::Move,
            2 => TouchPhase::Up,
            3 => TouchPhase::Cancel,
            _ => return,
        };
        self.framebuffer.touch(phase, x, y);
    }

    /// Routes crown units through Cranpose's existing detent-to-scroll conversion.
    /// The native host supplies the timestamp in milliseconds since launch.
    pub fn crown(&mut self, delta: f32, elapsed_millis: u64) -> bool {
        self.framebuffer.rotary(delta, elapsed_millis)
    }

    /// Suspends frame production and cancels held touches when leaving the foreground.
    pub fn set_active(&mut self, active: bool) {
        self.framebuffer.set_active(active);
    }
}
