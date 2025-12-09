//! WGPU renderer backend for GPU-accelerated 2D rendering.
//!
//! This renderer uses WGPU for cross-platform GPU support across
//! desktop (Windows/Mac/Linux), web (WebGPU), and mobile (Android/iOS).

mod pipeline;
mod render;
mod scene;
mod shaders;

pub use scene::{ClickAction, DrawShape, HitRegion, Scene, TextDraw};

use compose_render_common::{RenderScene, Renderer};
use compose_ui::{set_text_measurer, LayoutTree, TextMeasurer};
use compose_ui_graphics::Size;
use glyphon::{Attrs, Buffer, FontSystem, Metrics, Shaping};
use lru::LruCache;
use render::GpuRenderer;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};

#[derive(Debug)]
pub enum WgpuRendererError {
    Layout(String),
    Wgpu(String),
}

/// Unified hash key for text caching - shared between measurement and rendering
/// Only content + scale matter, not position
#[derive(Clone)]
pub(crate) struct TextCacheKey {
    text: String,
    scale_bits: u32, // f32 as bits for hashing
}

impl TextCacheKey {
    fn new(text: &str, font_size: f32) -> Self {
        Self {
            text: text.to_string(),
            scale_bits: font_size.to_bits(),
        }
    }
}

impl Hash for TextCacheKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.text.hash(state);
        self.scale_bits.hash(state);
    }
}

impl PartialEq for TextCacheKey {
    fn eq(&self, other: &Self) -> bool {
        self.text == other.text && self.scale_bits == other.scale_bits
    }
}

impl Eq for TextCacheKey {}

/// Cached text buffer shared between measurement and rendering
pub(crate) struct SharedTextBuffer {
    pub(crate) buffer: Buffer,
    text: String,
    font_size: f32,
    /// Cached size to avoid recalculating on every access
    cached_size: Option<Size>,
}

impl SharedTextBuffer {
    /// Ensure the buffer has the correct text and font_size, only reshaping if needed
    pub(crate) fn ensure(
        &mut self,
        font_system: &mut FontSystem,
        text: &str,
        font_size: f32,
        attrs: Attrs,
    ) {
        let text_changed = self.text != text;
        let font_changed = (self.font_size - font_size).abs() > 0.1;

        // Only reshape if something actually changed
        if !text_changed && !font_changed {
            return; // Nothing changed, skip reshape
        }

        // Set metrics and size for unlimited layout
        let metrics = Metrics::new(font_size, font_size * 1.4);
        self.buffer.set_metrics(font_system, metrics);
        self.buffer.set_size(font_system, Some(f32::MAX), Some(f32::MAX));

        // Set text and shape
        self.buffer
            .set_text(font_system, text, &attrs, Shaping::Advanced);
        self.buffer.shape_until_scroll(font_system, false);

        // Update cached values
        self.text.clear();
        self.text.push_str(text);
        self.font_size = font_size;
        self.cached_size = None; // Invalidate size cache
    }

    /// Get or calculate the size of the shaped text
    pub(crate) fn size(&mut self, font_size: f32) -> Size {
        if let Some(size) = self.cached_size {
            return size;
        }

        // Calculate size from buffer
        let mut max_width = 0.0f32;
        let layout_runs = self.buffer.layout_runs();
        for run in layout_runs {
            max_width = max_width.max(run.line_w);
        }
        let total_height = self.buffer.lines.len() as f32 * font_size * 1.4;

        let size = Size {
            width: max_width,
            height: total_height,
        };

        self.cached_size = Some(size);
        size
    }
}

/// Shared cache for text buffers used by both measurement and rendering
pub(crate) type SharedTextCache = Arc<Mutex<HashMap<TextCacheKey, SharedTextBuffer>>>;

/// Trim text cache if it exceeds MAX_CACHE_ITEMS.
/// Removes the oldest half of entries when limit is reached.
fn trim_text_cache(cache: &mut HashMap<TextCacheKey, SharedTextBuffer>) {
    if cache.len() > MAX_CACHE_ITEMS {
        let target_size = MAX_CACHE_ITEMS / 2;
        let to_remove = cache.len() - target_size;

        // Remove oldest entries (arbitrary keys from the front)
        let keys_to_remove: Vec<TextCacheKey> = cache.keys().take(to_remove).cloned().collect();

        for key in keys_to_remove {
            cache.remove(&key);
        }

        log::debug!(
            "Trimmed text cache from {} to {} entries",
            cache.len() + to_remove,
            cache.len()
        );
    }
}

/// Maximum number of cached text buffers before trimming occurs
const MAX_CACHE_ITEMS: usize = 256;

/// WGPU-based renderer for GPU-accelerated 2D rendering.
///
/// This renderer supports:
/// - GPU-accelerated shape rendering (rectangles, rounded rectangles)
/// - Gradients (solid, linear, radial)
/// - GPU text rendering via glyphon
/// - Cross-platform support (Desktop, Web, Mobile)
pub struct WgpuRenderer {
    scene: Scene,
    gpu_renderer: Option<GpuRenderer>,
    font_system: Arc<Mutex<FontSystem>>,
    /// Shared text buffer cache used by both measurement and rendering
    text_cache: SharedTextCache,
    /// Root scale factor for text rendering (use for density scaling)
    root_scale: f32,
}

impl WgpuRenderer {
    /// Create a new WGPU renderer with the specified font data.
    ///
    /// This is the recommended constructor for applications.
    /// Call `init_gpu` before rendering.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let font_light = include_bytes!("path/to/font-light.ttf");
    /// let font_regular = include_bytes!("path/to/font-regular.ttf");
    /// let renderer = WgpuRenderer::new_with_fonts(&[font_light, font_regular]);
    /// ```
    pub fn new_with_fonts(fonts: &[&[u8]]) -> Self {
        let mut font_system = FontSystem::new();

        // On Android, DO NOT load system fonts
        // Modern Android uses Variable Fonts for Roboto which can cause
        // rasterization corruption or font ID conflicts with glyphon.
        // Use only our bundled static Roboto fonts for consistent rendering.
        #[cfg(target_os = "android")]
        {
            log::info!("Skipping Android system fonts - using application-provided fonts");
            // font_system.db_mut().load_fonts_dir("/system/fonts");  // DISABLED
        }

        // Load application-provided fonts
        for (i, font_data) in fonts.iter().enumerate() {
            log::info!("Loading font #{}, size: {} bytes", i, font_data.len());
            font_system.db_mut().load_font_data(font_data.to_vec());
        }

        let face_count = font_system.db().faces().count();
        log::info!("Total font faces loaded: {}", face_count);

        if face_count == 0 {
            log::error!("No fonts loaded! Text rendering will fail!");
        }

        let font_system = Arc::new(Mutex::new(font_system));

        // Create shared text cache for both measurement and rendering
        let text_cache = Arc::new(Mutex::new(HashMap::new()));

        let text_measurer = WgpuTextMeasurer::new(font_system.clone(), text_cache.clone());
        set_text_measurer(text_measurer.clone());

        Self {
            scene: Scene::new(),
            gpu_renderer: None,
            font_system,
            text_cache,
            root_scale: 1.0,
        }
    }

    /// Create a new WGPU renderer without any fonts.
    ///
    /// **Warning:** This is for internal use only. Applications should use `new_with_fonts()`.
    /// Text rendering will fail without fonts.
    pub fn new() -> Self {
        let font_system = FontSystem::new();
        let font_system = Arc::new(Mutex::new(font_system));
        let text_cache = Arc::new(Mutex::new(HashMap::new()));

        let text_measurer = WgpuTextMeasurer::new(font_system.clone(), text_cache.clone());
        set_text_measurer(text_measurer.clone());

        Self {
            scene: Scene::new(),
            gpu_renderer: None,
            font_system,
            text_cache,
            root_scale: 1.0,
        }
    }

    /// Initialize GPU resources with a WGPU device and queue.
    pub fn init_gpu(
        &mut self,
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        surface_format: wgpu::TextureFormat,
    ) {
        self.gpu_renderer = Some(GpuRenderer::new(
            device,
            queue,
            surface_format,
            self.font_system.clone(),
            self.text_cache.clone(),
        ));
    }

    /// Set root scale factor for text rendering (e.g., density scaling on Android)
    pub fn set_root_scale(&mut self, scale: f32) {
        self.root_scale = scale;
    }

    /// Render the scene to a texture view.
    pub fn render(
        &mut self,
        view: &wgpu::TextureView,
        width: u32,
        height: u32,
    ) -> Result<(), WgpuRendererError> {
        if let Some(gpu_renderer) = &mut self.gpu_renderer {
            gpu_renderer
                .render(
                    view,
                    &self.scene.shapes,
                    &self.scene.texts,
                    width,
                    height,
                    self.root_scale,
                )
                .map_err(WgpuRendererError::Wgpu)
        } else {
            Err(WgpuRendererError::Wgpu(
                "GPU renderer not initialized. Call init_gpu() first.".to_string(),
            ))
        }
    }

    /// Get access to the WGPU device (for surface configuration).
    pub fn device(&self) -> &wgpu::Device {
        self.gpu_renderer
            .as_ref()
            .map(|r| &*r.device)
            .expect("GPU renderer not initialized")
    }
}

impl Default for WgpuRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer for WgpuRenderer {
    type Scene = Scene;
    type Error = WgpuRendererError;

    fn scene(&self) -> &Self::Scene {
        &self.scene
    }

    fn scene_mut(&mut self) -> &mut Self::Scene {
        &mut self.scene
    }

    fn rebuild_scene(
        &mut self,
        layout_tree: &LayoutTree,
        _viewport: Size,
    ) -> Result<(), Self::Error> {
        self.scene.clear();
        // Build scene in logical dp - scaling happens in GPU vertex upload
        pipeline::render_layout_tree(layout_tree.root(), &mut self.scene);
        Ok(())
    }
}

// Text measurer implementation for WGPU

#[derive(Clone)]
struct WgpuTextMeasurer {
    font_system: Arc<Mutex<FontSystem>>,
    /// Size-only cache for ultra-fast lookups
    size_cache: Arc<Mutex<LruCache<(String, i32), Size>>>,
    /// Shared buffer cache used by both measurement and rendering
    text_cache: SharedTextCache,
}

impl WgpuTextMeasurer {
    fn new(font_system: Arc<Mutex<FontSystem>>, text_cache: SharedTextCache) -> Self {
        Self {
            font_system,
            size_cache: Arc::new(Mutex::new(LruCache::new(NonZeroUsize::new(64).unwrap()))),
            text_cache,
        }
    }
}

// Base font size in logical units (dp) - shared between measurement and rendering
pub(crate) const BASE_FONT_SIZE: f32 = 14.0;

impl TextMeasurer for WgpuTextMeasurer {
    fn measure(&self, text: &str) -> compose_ui::TextMetrics {
        let size_key = (text.to_string(), (BASE_FONT_SIZE * 100.0) as i32);

        // Check size cache first (fastest path)
        {
            let mut cache = self.size_cache.lock().unwrap();
            if let Some(size) = cache.get(&size_key) {
                return compose_ui::TextMetrics {
                    width: size.width,
                    height: size.height,
                };
            }
        }

        // Get or create text buffer
        let cache_key = TextCacheKey::new(text, BASE_FONT_SIZE);
        let mut font_system = self.font_system.lock().unwrap();
        let mut text_cache = self.text_cache.lock().unwrap();

        // Get or create buffer and calculate size
        let size = {
            let buffer = text_cache.entry(cache_key).or_insert_with(|| {
                let buffer = Buffer::new(
                    &mut font_system,
                    Metrics::new(BASE_FONT_SIZE, BASE_FONT_SIZE * 1.4),
                );
                SharedTextBuffer {
                    buffer,
                    text: String::new(),
                    font_size: 0.0,
                    cached_size: None,
                }
            });

            // Ensure buffer has the correct text
            buffer.ensure(&mut font_system, text, BASE_FONT_SIZE, Attrs::new());

            // Calculate size if not cached
            buffer.size(BASE_FONT_SIZE)
        };

        // Trim cache if needed (after we're done with buffer reference)
        trim_text_cache(&mut text_cache);

        drop(font_system);
        drop(text_cache);

        // Cache the size result
        let mut size_cache = self.size_cache.lock().unwrap();
        size_cache.put(size_key, size);

        compose_ui::TextMetrics {
            width: size.width,
            height: size.height,
        }
    }
}
