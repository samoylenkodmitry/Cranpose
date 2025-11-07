//! WGPU renderer backend for GPU-accelerated 2D rendering.
//!
//! This renderer uses WGPU for cross-platform GPU support across
//! desktop (Windows/Mac/Linux), web (WebGPU), and mobile (Android/iOS).

mod font;
mod pipeline;
mod render;
mod scene;
mod shaders;
mod text_cache;

pub use scene::{ClickAction, DrawShape, HitRegion, Scene, TextDraw};

use compose_render_common::{RenderScene, Renderer};
use compose_ui::{set_text_measurer, LayoutTree, TextMeasurer};
use compose_ui_graphics::Size;
use font::{
    create_font_system, detect_preferred_font, fallback_text_attrs, primary_text_attrs,
    PreferredFont, DEFAULT_FONT_SIZE, DEFAULT_LINE_HEIGHT,
};
use glyphon::{FontSystem, Metrics};
use lru::LruCache;
use render::GpuRenderer;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};
use text_cache::{
    grow_text_cache, text_metrics_from_buffer, CachedTextBuffer, TextCacheKey,
    TEXT_CACHE_INITIAL_CAPACITY,
};

#[derive(Debug)]
pub enum WgpuRendererError {
    Layout(String),
    Wgpu(String),
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
    preferred_font: Option<PreferredFont>,
    render_cache: Arc<Mutex<LruCache<TextCacheKey, Box<CachedTextBuffer>>>>,
}

impl WgpuRenderer {
    /// Create a new WGPU renderer without GPU resources.
    /// Call `init_gpu` before rendering.
    pub fn new() -> Self {
        let base_font_system = create_font_system();
        let preferred_font = detect_preferred_font(&base_font_system);
        if let Some(font) = &preferred_font {
            log::info!(
                "GPU text font: using '{}' (weight {})",
                font.family,
                font.weight.0
            );
        } else {
            log::warn!("GPU text font: falling back to system sans-serif family");
        }

        let font_system = Arc::new(Mutex::new(base_font_system));
        let measure_cache = Arc::new(Mutex::new(LruCache::new(
            NonZeroUsize::new(TEXT_CACHE_INITIAL_CAPACITY).unwrap(),
        )));
        let render_cache = Arc::new(Mutex::new(LruCache::new(
            NonZeroUsize::new(TEXT_CACHE_INITIAL_CAPACITY).unwrap(),
        )));
        let text_measurer = WgpuTextMeasurer::new(
            font_system.clone(),
            measure_cache.clone(),
            preferred_font.clone(),
        );
        set_text_measurer(text_measurer.clone());

        Self {
            scene: Scene::new(),
            gpu_renderer: None,
            font_system,
            preferred_font,
            render_cache,
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
            self.preferred_font.clone(),
            self.render_cache.clone(),
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
    text_cache: Arc<Mutex<LruCache<TextCacheKey, Box<CachedTextBuffer>>>>,
    preferred_font: Option<PreferredFont>,
}

impl WgpuTextMeasurer {
    fn new(
        font_system: Arc<Mutex<FontSystem>>,
        text_cache: Arc<Mutex<LruCache<TextCacheKey, Box<CachedTextBuffer>>>>,
        preferred_font: Option<PreferredFont>,
    ) -> Self {
        Self {
            font_system,
            text_cache,
            preferred_font,
        }
    }
}

impl TextMeasurer for WgpuTextMeasurer {
    fn measure(&self, text: &str) -> compose_ui::TextMetrics {
        let font_size = DEFAULT_FONT_SIZE;
        let text_scale = 1.0f32;
        let key = TextCacheKey::new(text, text_scale);

        {
            let cache = self.text_cache.lock().unwrap();
            if let Some(entry) = cache.peek(&key) {
                return text_metrics_from_buffer(entry.as_ref());
            }
        }

        let mut font_system = self.font_system.lock().unwrap();
        let metrics = Metrics::new(font_size, DEFAULT_LINE_HEIGHT);
        let primary_attrs = primary_text_attrs(self.preferred_font.as_ref());
        let fallback_attrs = fallback_text_attrs(self.preferred_font.as_ref());
        let cached = CachedTextBuffer::new(
            &mut font_system,
            metrics,
            key.scale_key(),
            f32::MAX,
            text,
            primary_attrs,
            fallback_attrs,
        );
        let metrics = text_metrics_from_buffer(&cached);
        drop(font_system);

        {
            let mut cache = self.text_cache.lock().unwrap();
            if cache.len() == cache.cap().get() {
                grow_text_cache(&mut cache);
            }
            cache.put(key, Box::new(cached));
        }

        metrics
    }
}
