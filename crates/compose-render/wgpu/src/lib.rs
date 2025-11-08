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
    /// Returns true if reshaping occurred
    pub(crate) fn ensure(
        &mut self,
        font_system: &mut FontSystem,
        text: &str,
        font_size: f32,
        attrs: Attrs,
    ) -> bool {
        // Check if anything changed that requires reshaping
        if self.text == text && self.font_size == font_size {
            return false; // No reshaping needed!
        }

        // Something changed, need to reshape
        let metrics = Metrics::new(font_size, font_size * 1.4);
        self.buffer.set_metrics(font_system, metrics);
        self.buffer
            .set_text(font_system, text, attrs, Shaping::Advanced);
        self.buffer.shape_until_scroll(font_system);

        // Update cached values
        self.text.clear();
        self.text.push_str(text);
        self.font_size = font_size;
        self.cached_size = None; // Invalidate size cache

        true
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
}

impl WgpuRenderer {
    /// Create a new WGPU renderer without GPU resources.
    /// Call `init_gpu` before rendering.
    pub fn new() -> Self {
        let mut font_system = FontSystem::new();

        // Load Roboto font into the system
        let font_data = include_bytes!("../../../../apps/desktop-demo/assets/Roboto-Light.ttf");
        font_system.db_mut().load_font_data(font_data.to_vec());

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

    /// Render the scene to a texture view.
    pub fn render(
        &mut self,
        view: &wgpu::TextureView,
        width: u32,
        height: u32,
    ) -> Result<(), WgpuRendererError> {
        if let Some(gpu_renderer) = &mut self.gpu_renderer {
            gpu_renderer
                .render(view, &self.scene.shapes, &self.scene.texts, width, height)
                .map_err(|e| WgpuRendererError::Wgpu(e))
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

impl TextMeasurer for WgpuTextMeasurer {
    fn measure(&self, text: &str) -> compose_ui::TextMetrics {
        let font_size = 14.0; // Default font size
        let size_key = (text.to_string(), (font_size * 100.0) as i32);

        // Check size cache first (fastest path)
        {
            let mut cache = self.size_cache.lock().unwrap();
            if let Some(size) = cache.get(&size_key) {
                // Size cache HIT - fastest path!
                return compose_ui::TextMetrics {
                    width: size.width,
                    height: size.height,
                };
            }
        }

        // Need to measure - check shared text cache
        let cache_key = TextCacheKey::new(text, font_size);
        let mut font_system = self.font_system.lock().unwrap();
        let mut text_cache = self.text_cache.lock().unwrap();

        // Get or create cached buffer and measure it
        let size = if let Some(cached) = text_cache.get_mut(&cache_key) {
            // Shared cache hit - use ensure() to only reshape if needed
            cached.ensure(&mut font_system, text, font_size, Attrs::new());
            cached.size(font_size)
        } else {
            // Cache miss - create new buffer and add to shared cache
            let mut new_buffer =
                Buffer::new(&mut font_system, Metrics::new(font_size, font_size * 1.4));
            new_buffer.set_size(&mut font_system, f32::MAX, f32::MAX);
            new_buffer.set_text(&mut font_system, text, Attrs::new(), Shaping::Advanced);
            new_buffer.shape_until_scroll(&mut font_system);

            let mut shared_buffer = SharedTextBuffer {
                buffer: new_buffer,
                text: text.to_string(),
                font_size,
                cached_size: None,
            };
            let size = shared_buffer.size(font_size);
            text_cache.insert(cache_key, shared_buffer);
            size
        };

        // Cache the size result
        let mut size_cache = self.size_cache.lock().unwrap();
        size_cache.put(size_key, size);

        compose_ui::TextMetrics {
            width: size.width,
            height: size.height,
        }
    }
}
