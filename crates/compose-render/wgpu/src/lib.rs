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

pub(crate) const BASE_FONT_SIZE_DP: f32 = 14.0;
const TEXT_CACHE_MAX_ENTRIES: usize = 256;

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
    /// Ensure the buffer has the correct text and font size; reshape only when needed
    pub(crate) fn ensure(
        &mut self,
        font_system: &mut FontSystem,
        text: &str,
        font_size: f32,
        attrs: Attrs,
    ) {
        let text_changed = self.text != text;
        let font_changed = (self.font_size - font_size).abs() > 0.1;

        if !text_changed && !font_changed {
            return;
        }

        let metrics = Metrics::new(font_size, font_size * 1.4);
        self.buffer.set_metrics(font_system, metrics);
        self.buffer.set_size(font_system, f32::MAX, f32::MAX);
        self.buffer
            .set_text(font_system, text, attrs, Shaping::Advanced);
        self.buffer.shape_until_scroll(font_system);

        self.text.clear();
        self.text.push_str(text);
        self.font_size = font_size;
        self.cached_size = None;
    }
}

/// Shared cache for text buffers used by both measurement and rendering
pub(crate) type SharedTextCache = Arc<Mutex<HashMap<TextCacheKey, SharedTextBuffer>>>;

fn enforce_text_cache_limit(cache: &mut HashMap<TextCacheKey, SharedTextBuffer>) {
    if cache.len() >= TEXT_CACHE_MAX_ENTRIES {
        if let Some(key) = cache.keys().next().cloned() {
            cache.remove(&key);
        }
    }
}

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
    root_scale: Arc<Mutex<f32>>,
}

impl WgpuRenderer {
    /// Create a new WGPU renderer without GPU resources.
    /// Call `init_gpu` before rendering.
    pub fn new() -> Self {
        let mut font_system = FontSystem::new();

        // On Android, DO NOT load system fonts
        // Modern Android uses Variable Fonts for Roboto which can cause
        // rasterization corruption or font ID conflicts with glyphon.
        // Use only our bundled static Roboto fonts for consistent rendering.
        #[cfg(target_os = "android")]
        {
            log::info!("Skipping Android system fonts - using bundled static Roboto only");
            // font_system.db_mut().load_fonts_dir("/system/fonts");  // DISABLED
        }

        // Load embedded Roboto fonts (static versions, not Variable Fonts)
        let font_light = include_bytes!("../../../../assets/Roboto-Light.ttf");
        let font_regular = include_bytes!("../../../../assets/Roboto-Regular.ttf");

        log::info!(
            "Loading Roboto Light font, size: {} bytes",
            font_light.len()
        );
        font_system.db_mut().load_font_data(font_light.to_vec());

        log::info!(
            "Loading Roboto Regular font, size: {} bytes",
            font_regular.len()
        );
        font_system.db_mut().load_font_data(font_regular.to_vec());

        let face_count = font_system.db().faces().count();
        log::info!("Total font faces loaded: {}", face_count);

        if face_count == 0 {
            log::error!("No fonts loaded! Text rendering will fail!");
        }

        let font_system = Arc::new(Mutex::new(font_system));

        let root_scale = Arc::new(Mutex::new(1.0));

        // Create shared text cache for both measurement and rendering
        let text_cache = Arc::new(Mutex::new(HashMap::new()));

        let text_measurer =
            WgpuTextMeasurer::new(font_system.clone(), text_cache.clone(), root_scale.clone());
        set_text_measurer(text_measurer.clone());

        Self {
            scene: Scene::new(),
            gpu_renderer: None,
            font_system,
            text_cache,
            root_scale,
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
        if let Ok(mut current) = self.root_scale.lock() {
            *current = scale;
        }
    }

    fn root_scale(&self) -> f32 {
        *self.root_scale.lock().expect("root scale lock poisoned")
    }

    /// Render the scene to a texture view.
    pub fn render(
        &mut self,
        view: &wgpu::TextureView,
        width: u32,
        height: u32,
    ) -> Result<(), WgpuRendererError> {
        let root_scale = self.root_scale();

        if let Some(gpu_renderer) = &mut self.gpu_renderer {
            gpu_renderer
                .render(
                    view,
                    &self.scene.shapes,
                    &self.scene.texts,
                    width,
                    height,
                    root_scale,
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
    root_scale: Arc<Mutex<f32>>,
}

impl WgpuTextMeasurer {
    fn new(
        font_system: Arc<Mutex<FontSystem>>,
        text_cache: SharedTextCache,
        root_scale: Arc<Mutex<f32>>,
    ) -> Self {
        Self {
            font_system,
            size_cache: Arc::new(Mutex::new(LruCache::new(NonZeroUsize::new(64).unwrap()))),
            text_cache,
            root_scale,
        }
    }

    fn root_scale(&self) -> f32 {
        *self.root_scale.lock().expect("root scale lock poisoned")
    }
}

impl TextMeasurer for WgpuTextMeasurer {
    fn measure(&self, text: &str) -> compose_ui::TextMetrics {
        let font_size_px = BASE_FONT_SIZE_DP * self.root_scale();
        let size_key = (text.to_string(), (BASE_FONT_SIZE_DP * 100.0) as i32);

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

        let cache_key = TextCacheKey::new(text, font_size_px);

        let mut font_system = self.font_system.lock().unwrap();
        let mut text_cache = self.text_cache.lock().unwrap();

        if !text_cache.contains_key(&cache_key) {
            enforce_text_cache_limit(&mut text_cache);
        }

        let cached = text_cache
            .entry(cache_key)
            .or_insert_with(|| SharedTextBuffer {
                buffer: Buffer::new(
                    &mut font_system,
                    Metrics::new(font_size_px, font_size_px * 1.4),
                ),
                text: String::new(),
                font_size: 0.0,
                cached_size: None,
            });

        cached.ensure(&mut font_system, text, font_size_px, Attrs::new());

        if cached.cached_size.is_none() {
            let mut max_width = 0.0f32;
            for run in cached.buffer.layout_runs() {
                max_width = max_width.max(run.line_w);
            }
            let line_height = cached.buffer.metrics().line_height;
            let total_height = cached.buffer.lines.len() as f32 * line_height;
            cached.cached_size = Some(Size {
                width: max_width,
                height: total_height,
            });
        }

        let size_px = cached.cached_size.expect("cached_size just set");
        let scale = self.root_scale();
        let size_dp = Size {
            width: size_px.width / scale,
            height: size_px.height / scale,
        };

        let mut size_cache = self.size_cache.lock().unwrap();
        size_cache.put(size_key, size_dp);

        compose_ui::TextMetrics {
            width: size_dp.width,
            height: size_dp.height,
        }
    }
}
